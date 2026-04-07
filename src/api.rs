use crate::app::*;
use crate::db::Db;

/// 채팅 메시지 전송
pub async fn send_chat(access_token: &str, message: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/open/v1/chats/send", BASE_URL))
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "message": message }))
        .send()
        .await
        .map_err(|e| format!("채팅 전송 실패: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("채팅 전송 실패 ({}): {}", status, body));
    }

    let api_resp: ApiResponse<SendMessageResponse> =
        serde_json::from_str(&body).map_err(|e| format!("{e}"))?;

    Ok(api_resp.content.map(|c| c.message_id).unwrap_or_default())
}

/// 라이브 상태 조회 (공개 API, 인증 불필요)
pub async fn get_live_status(channel_id: &str) -> Result<LiveStatus, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v1/{}/live-status", BASE_URL, channel_id))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    let body = resp.text().await.unwrap_or_default();
    let api_resp: ApiResponse<LiveStatus> =
        serde_json::from_str(&body).map_err(|e| format!("{e}"))?;

    api_resp.content.ok_or_else(|| "라이브 상태 없음".into())
}

/// 라이브 설정 조회 (제목, 카테고리)
pub async fn get_live_setting(access_token: &str) -> Result<LiveSetting, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/open/v1/lives/setting", BASE_URL))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    let body = resp.text().await.unwrap_or_default();
    let api_resp: ApiResponse<LiveSetting> =
        serde_json::from_str(&body).map_err(|e| format!("{e}"))?;

    api_resp.content.ok_or_else(|| "라이브 설정 없음".into())
}

/// WebSocket 세션 생성
pub async fn create_session(access_token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/open/v1/sessions/auth", BASE_URL))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("세션 생성 실패: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("세션 생성 실패 ({}): {}", status, body));
    }

    let api_resp: ApiResponse<SessionAuth> =
        serde_json::from_str(&body).map_err(|e| format!("{e}"))?;

    api_resp.content.map(|c| c.url).ok_or_else(|| "세션 URL 없음".into())
}

/// 이벤트 구독
pub async fn subscribe_event(
    access_token: &str,
    event_type: &str,
    session_key: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/open/v1/sessions/events/subscribe/{}?sessionKey={}",
            BASE_URL, event_type, session_key
        ))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("이벤트 구독 실패: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("이벤트 구독 실패 ({}): {}", status, body));
    }

    Ok(())
}

/// 팔로워 목록에서 특정 채널 ID의 팔로우 날짜 찾기
pub async fn find_follower_date(
    access_token: &str,
    target_channel_id: &str,
) -> Result<Option<String>, String> {
    let client = reqwest::Client::new();
    let mut page = 0;

    loop {
        let resp = client
            .get(format!("{}/open/v1/channels/followers", BASE_URL))
            .bearer_auth(access_token)
            .query(&[("page", &page.to_string()), ("size", &"50".to_string())])
            .send()
            .await
            .map_err(|e| format!("{e}"))?;

        let body = resp.text().await.unwrap_or_default();

        // 팔로워 목록 파싱
        let api_resp: ApiResponse<PagedFollowers> =
            serde_json::from_str(&body).map_err(|e| format!("{e}"))?;

        if let Some(content) = api_resp.content {
            if content.data.is_empty() {
                return Ok(None);
            }
            for f in &content.data {
                if f.channel_id == target_channel_id {
                    return Ok(Some(f.created_date.clone()));
                }
            }
            // 마지막 페이지 여부: 데이터가 size보다 적으면 끝
            if content.data.len() < 50 {
                return Ok(None);
            }
            page += 1;
            // 최대 10페이지까지만 (500명)
            if page >= 10 {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PagedFollowers {
    data: Vec<FollowerInfo>,
}

/// 라이브 설정 업데이트 (방제, 카테고리, 태그)
pub async fn update_live_setting(
    access_token: &str,
    title: Option<&str>,
    category_id: Option<&str>,
    tags: Option<&[String]>,
) -> Result<(), String> {
    let mut body = serde_json::Map::new();
    if let Some(t) = title {
        body.insert("defaultLiveTitle".into(), serde_json::Value::String(t.to_string()));
    }
    if let Some(c) = category_id {
        if c.is_empty() {
            body.insert("categoryId".into(), serde_json::Value::Null);
        } else {
            body.insert("categoryId".into(), serde_json::Value::String(c.to_string()));
        }
    }
    if let Some(t) = tags {
        body.insert("tags".into(), serde_json::to_value(t).unwrap_or_default());
    }
    let client = reqwest::Client::new();
    let resp = client
        .patch(format!("{}/open/v1/lives/setting", BASE_URL))
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("라이브 설정 변경 실패 ({}): {}", status, body));
    }
    Ok(())
}

/// 채팅 공지 등록
pub async fn register_chat_notice(access_token: &str, message: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/open/v1/chats/notice", BASE_URL))
        .bearer_auth(access_token)
        .json(&serde_json::json!({ "message": message }))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("공지 등록 실패 ({}): {}", status, body));
    }
    Ok(())
}

/// 카테고리 검색
pub async fn search_categories(
    client_id: &str,
    client_secret: &str,
    keyword: &str,
) -> Result<Vec<CategorySearchResult>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/open/v1/categories/search", BASE_URL))
        .header("Client-Id", client_id)
        .header("Client-Secret", client_secret)
        .query(&[("keyword", keyword), ("size", "50")])
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    let body = resp.text().await.unwrap_or_default();

    #[derive(serde::Deserialize)]
    struct CatContent { data: Vec<CategorySearchResult> }

    let api_resp: ApiResponse<CatContent> =
        serde_json::from_str(&body).map_err(|e| format!("{e}"))?;
    Ok(api_resp.content.map(|c| c.data).unwrap_or_default())
}

/// 채널 정보 갱신 루프
pub async fn channel_info_loop(shared: Shared, _db: Db) {
    loop {
        let (channel_id, access_token) = {
            let st = shared.lock().unwrap();
            if !st.bot_running || st.bot_should_stop {
                return;
            }
            (
                st.channel_id.clone().unwrap_or_default(),
                st.access_token.clone().unwrap_or_default(),
            )
        };

        if !channel_id.is_empty() {
            // 라이브 상태
            if let Ok(status) = get_live_status(&channel_id).await {
                let mut st = shared.lock().unwrap();
                st.is_live = status.is_live;
                if let Some(t) = &status.title {
                    st.live_title = t.clone();
                }
                st.opened_at = status.opened_at;
            }

            // 라이브 설정 (카테고리)
            if !access_token.is_empty() {
                if let Ok(setting) = get_live_setting(&access_token).await {
                    let mut st = shared.lock().unwrap();
                    if let Some(t) = &setting.default_live_title {
                        if st.live_title.is_empty() {
                            st.live_title = t.clone();
                        }
                    }
                    st.category = setting
                        .category
                        .and_then(|c| c.category_value)
                        .unwrap_or_else(|| "없음".into());
                }
            }

        }

        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

/// WebSocket URL에서 sessionKey 추출
pub fn extract_session_key(ws_url: &str) -> Option<String> {
    url::Url::parse(ws_url)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == "sessionKey")
        .map(|(_, v)| v.to_string())
}
