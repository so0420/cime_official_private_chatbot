use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::api;
use crate::app::*;
use crate::bot;
use crate::db::Db;

/// WebSocket 이벤트 수신 메인 루프
pub async fn event_loop(shared: Shared, db: Db) {
    loop {
        {
            let st = shared.lock().unwrap();
            if st.bot_should_stop || !st.bot_running {
                return;
            }
        }

        if let Err(e) = run_session(&shared, &db).await {
            let mut st = shared.lock().unwrap();
            st.ws_connected = false;
            st.log(&format!("[WS] 세션 오류: {e}"));
            if st.bot_should_stop {
                return;
            }
        }

        // 재연결 대기
        {
            let st = shared.lock().unwrap();
            if st.bot_should_stop {
                return;
            }
        }

        {
            let mut st = shared.lock().unwrap();
            st.log("[WS] 5초 후 재연결...");
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn run_session(shared: &Shared, db: &Db) -> Result<(), String> {
    let access_token = {
        let st = shared.lock().unwrap();
        st.access_token.clone().unwrap_or_default()
    };
    if access_token.is_empty() {
        return Err("Access token 없음".into());
    }

    // 세션 생성
    let ws_url = api::create_session(&access_token).await?;
    let session_key = api::extract_session_key(&ws_url)
        .ok_or_else(|| "sessionKey 추출 실패".to_string())?;

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("[WS] 세션 생성 완료 (key: {}...)", &session_key[..8.min(session_key.len())]));
    }

    // WebSocket 연결
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("WebSocket 연결 실패: {e}"))?;

    let (mut write, mut read) = ws_stream.split();

    {
        let mut st = shared.lock().unwrap();
        st.ws_connected = true;
        st.log("[WS] WebSocket 연결됨");
    }

    // 이벤트 구독 (chat, donation)
    api::subscribe_event(&access_token, "chat", &session_key).await?;
    {
        let mut st = shared.lock().unwrap();
        st.log("[WS] 채팅 이벤트 구독 완료");
    }

    api::subscribe_event(&access_token, "donation", &session_key).await?;
    {
        let mut st = shared.lock().unwrap();
        st.log("[WS] 후원 이벤트 구독 완료");
    }

    api::subscribe_event(&access_token, "subscription", &session_key).await?;
    {
        let mut st = shared.lock().unwrap();
        st.log("[WS] 구독 이벤트 구독 완료");
    }

    // Ping 루프
    let ping_task = tokio::spawn({
        let shared = shared.clone();
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                // ping은 write 접근 필요 → 여기서는 생략 (tungstenite 자동 처리)
                let st = shared.lock().unwrap();
                if st.bot_should_stop {
                    break;
                }
            }
        }
    });

    // 메시지 수신 루프
    while let Some(msg) = read.next().await {
        {
            let st = shared.lock().unwrap();
            if st.bot_should_stop {
                break;
            }
        }

        match msg {
            Ok(Message::Text(text)) => {
                handle_ws_message(shared, db, &text).await;
            }
            Ok(Message::Ping(data)) => {
                let _ = write.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                let mut st = shared.lock().unwrap();
                st.log("[WS] 서버에서 연결 종료");
                break;
            }
            Err(e) => {
                let mut st = shared.lock().unwrap();
                st.log(&format!("[WS] 수신 오류: {e}"));
                break;
            }
            _ => {}
        }
    }

    ping_task.abort();

    {
        let mut st = shared.lock().unwrap();
        st.ws_connected = false;
    }

    Ok(())
}

async fn handle_ws_message(shared: &Shared, db: &Db, text: &str) {
    // 원본 메시지 로그 (최대 300자)
    {
        let mut st = shared.lock().unwrap();
        let preview: String = text.chars().take(300).collect();
        st.log(&format!("[WS 수신] {}", preview));
    }

    let event: WsEvent = match serde_json::from_str(text) {
        Ok(e) => e,
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[WS] 파싱 실패: {}", e));
            return;
        }
    };

    match event.event.as_str() {
        "CHAT" => {
            if let Some(chat) = parse_chat_event(&event.data) {
                {
                    let mut st = shared.lock().unwrap();
                    st.log(&format!("[{}] {}", chat.sender_nickname, chat.content));
                }
                bot::handle_chat(shared, db, &chat).await;
            } else {
                let mut st = shared.lock().unwrap();
                st.log("[WS] CHAT 이벤트 파싱 실패");
            }
        }
        "DONATION" => {
            if let Some(donation) = parse_donation_event(&event.data) {
                bot::handle_donation(shared, db, &donation).await;
            } else {
                let mut st = shared.lock().unwrap();
                st.log("[WS] DONATION 이벤트 파싱 실패");
            }
        }
        "SUBSCRIPTION" => {
            if let Some(sub) = parse_subscription_event(&event.data) {
                bot::handle_subscription(shared, db, &sub).await;
            } else {
                let mut st = shared.lock().unwrap();
                st.log("[WS] SUBSCRIPTION 이벤트 파싱 실패");
            }
        }
        other => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[WS] 알 수 없는 이벤트: {}", other));
        }
    }
}

fn parse_chat_event(data: &serde_json::Value) -> Option<ChatEvent> {
    let channel_id = data.get("channelId")?.as_str()?.to_string();
    let sender_channel_id = data.get("senderChannelId")?.as_str()?.to_string();
    let profile = data.get("profile");
    let nickname = profile
        .and_then(|p| p.get("nickname"))
        .and_then(|n| n.as_str())
        .unwrap_or("알 수 없음")
        .to_string();
    let sender_slug = profile
        .and_then(|p| p.get("channelHandle"))
        .and_then(|h| h.as_str())
        .map(String::from);
    let content = data.get("content")?.as_str()?.to_string();

    Some(ChatEvent {
        channel_id,
        sender_channel_id,
        sender_nickname: nickname,
        sender_slug,
        content,
    })
}

fn parse_donation_event(data: &serde_json::Value) -> Option<DonationEvent> {
    let channel_id = data.get("channelId")?.as_str()?.to_string();
    let donator_channel_id = data
        .get("donatorChannelId")
        .and_then(|v| v.as_str())
        .map(String::from);
    let donator_nickname = data
        .get("donatorNickname")
        .and_then(|v| v.as_str())
        .map(String::from);
    // payAmount: 숫자 또는 문자열, 또는 amount 필드도 시도
    let pay_amount = data.get("payAmount")
        .or_else(|| data.get("amount"))
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0);
    let donation_text = data
        .get("donationText")
        .or_else(|| data.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Some(DonationEvent {
        channel_id,
        donator_channel_id,
        donator_nickname,
        pay_amount,
        donation_text,
    })
}

fn parse_subscription_event(data: &serde_json::Value) -> Option<crate::app::SubscriptionEvent> {
    let channel_id = data.get("channelId")?.as_str()?.to_string();
    let subscriber_channel_id = data.get("subscriberChannelId")?.as_str()?.to_string();
    let subscriber_channel_name = data.get("subscriberChannelName")?.as_str()?.to_string();
    let month = data.get("month")?.as_i64()? as i32;
    let tier_no = data.get("tierNo")?.as_i64()? as i32;
    let subscription_message = data
        .get("subscriptionMessage")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Some(crate::app::SubscriptionEvent {
        channel_id,
        subscriber_channel_id,
        subscriber_channel_name,
        month,
        tier_no,
        subscription_message,
    })
}
