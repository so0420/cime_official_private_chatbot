use crate::api;
use crate::app::*;
use crate::db::{self, Db};

/// 업타임 계산
pub fn calc_uptime(opened_at: &str) -> String {
    let start = match chrono::DateTime::parse_from_rfc3339(opened_at) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => {
            // ISO 8601 without timezone → assume UTC
            match chrono::NaiveDateTime::parse_from_str(opened_at, "%Y-%m-%dT%H:%M:%S%.f") {
                Ok(ndt) => ndt.and_utc(),
                Err(_) => return "알 수 없음".into(),
            }
        }
    };

    let total = (chrono::Utc::now() - start).num_seconds();
    if total < 0 {
        return "0초".into();
    }

    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;

    if h > 0 {
        format!("{}시간 {}분 {}초", h, m, s)
    } else if m > 0 {
        format!("{}분 {}초", m, s)
    } else {
        format!("{}초", s)
    }
}

/// 팔로우 기간 계산
pub fn calc_follow_duration(followed_at: &str) -> String {
    let start = match chrono::DateTime::parse_from_rfc3339(followed_at) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => match chrono::NaiveDateTime::parse_from_str(followed_at, "%Y-%m-%dT%H:%M:%S%.f") {
            Ok(ndt) => ndt.and_utc(),
            Err(_) => return "알 수 없음".into(),
        },
    };

    let total = (chrono::Utc::now() - start).num_seconds();
    if total < 0 {
        return "0분".into();
    }

    let d = total / 86400;
    let h = (total % 86400) / 3600;
    let m = (total % 3600) / 60;

    let mut parts = vec![];
    if d > 0 {
        parts.push(format!("{}일", d));
    }
    if h > 0 {
        parts.push(format!("{}시간", h));
    }
    if m > 0 {
        parts.push(format!("{}분", m));
    }
    if parts.is_empty() {
        "0분".into()
    } else {
        parts.join(" ")
    }
}

/// 특수 변수 치환
pub async fn replace_variables(
    template: &str,
    sender: &str,
    shared: &Shared,
    sender_channel_id: Option<&str>,
) -> String {
    let mut result = template.replace("<보낸사람>", sender);

    if result.contains("<업타임>") {
        let uptime = {
            let st = shared.lock().unwrap();
            match &st.opened_at {
                Some(opened) => calc_uptime(opened),
                None => "알 수 없음".into(),
            }
        };
        result = result.replace("<업타임>", &uptime);
    }

    if result.contains("<방제>") {
        let title = {
            let st = shared.lock().unwrap();
            if st.live_title.is_empty() {
                "알 수 없음".into()
            } else {
                st.live_title.clone()
            }
        };
        result = result.replace("<방제>", &title);
    }

    if result.contains("<카테고리>") {
        let cat = {
            let st = shared.lock().unwrap();
            if st.category.is_empty() {
                "없음".into()
            } else {
                st.category.clone()
            }
        };
        result = result.replace("<카테고리>", &cat);
    }

    if result.contains("<팔로우>") {
        let follow_str = if let Some(scid) = sender_channel_id {
            let token = {
                let st = shared.lock().unwrap();
                st.access_token.clone().unwrap_or_default()
            };
            if !token.is_empty() {
                match api::find_follower_date(&token, scid).await {
                    Ok(Some(date)) => calc_follow_duration(&date),
                    Ok(None) => "팔로우 안 함".into(),
                    Err(_) => "알 수 없음".into(),
                }
            } else {
                "알 수 없음".into()
            }
        } else {
            "알 수 없음".into()
        };
        result = result.replace("<팔로우>", &follow_str);
    }

    result
}

/// 채팅 이벤트 처리
pub async fn handle_chat(shared: &Shared, db: &Db, event: &ChatEvent) {
    let content = event.content.trim();
    let (trigger, args) = match content.split_once(' ') {
        Some((t, a)) => (t, a.trim()),
        None => (content, ""),
    };

    // 스트리머 명령어 처리 (인자가 있을 때만)
    if !args.is_empty() {
        let is_streamer = {
            let st = shared.lock().unwrap();
            st.channel_id.as_deref() == Some(&event.sender_channel_id)
        };
        if is_streamer {
            let handled = handle_streamer_command(shared, db, trigger, args).await;
            if handled { return; }
        }
    }

    // 기존 명령어 처리
    let command = db::find_command(db, content);
    let Some(command) = command else { return };

    let access_token = {
        let st = shared.lock().unwrap();
        st.access_token.clone().unwrap_or_default()
    };
    if access_token.is_empty() {
        return;
    }

    let reply = if command.is_attendance {
        // 출석체크
        let reset_hour: u32 = db::get_setting(db, SETTING_ATTENDANCE_RESET_HOUR)
            .parse()
            .unwrap_or(5);

        let (ok, count) =
            db::do_attendance(db, &event.sender_channel_id, &event.sender_nickname, reset_hour);

        let template = if ok {
            command.response.replace("<출석횟수>", &count.to_string())
        } else if !command.fail_response.is_empty() {
            command.fail_response.replace("<출석횟수>", &count.to_string())
        } else {
            format!(
                "{}님은 이미 오늘 출석체크를 하셨습니다. (총 {}회)",
                event.sender_nickname, count
            )
        };

        replace_variables(
            &template,
            &event.sender_nickname,
            shared,
            Some(&event.sender_channel_id),
        )
        .await
    } else {
        replace_variables(
            &command.response,
            &event.sender_nickname,
            shared,
            Some(&event.sender_channel_id),
        )
        .await
    };

    // 메시지 길이 제한 (100자)
    let reply = if reply.chars().count() > 100 {
        reply.chars().take(100).collect()
    } else {
        reply
    };

    match api::send_chat(&access_token, &reply).await {
        Ok(_) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[봇] {} → {}", content, reply));
        }
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[ERROR] 채팅 전송 실패: {e}"));
        }
    }
}

/// 스트리머 전용 명령어 처리. 처리했으면 true 반환.
async fn handle_streamer_command(shared: &Shared, db: &Db, trigger: &str, args: &str) -> bool {
    let (access_token, client_id, client_secret) = {
        let st = shared.lock().unwrap();
        (
            st.access_token.clone().unwrap_or_default(),
            st.client_id.clone(),
            st.client_secret.clone(),
        )
    };
    if access_token.is_empty() { return false; }

    match trigger {
        "!방제" if db::get_setting(db, SETTING_SCMD_TITLE) == "1" => {
            match api::update_live_setting(&access_token, Some(args), None, None).await {
                Ok(()) => {
                    {
                        let mut st = shared.lock().unwrap();
                        st.live_title = args.to_string();
                        st.log(&format!("[방제] 변경: {}", args));
                    }
                    let msg = format!("방제가 \"{}\"(으)로 변경되었습니다.", args);
                    let _ = api::send_chat(&access_token, &msg).await;
                }
                Err(e) => {
                    let mut st = shared.lock().unwrap();
                    st.log(&format!("[방제] 변경 실패: {e}"));
                }
            }
            true
        }
        "!공지" if db::get_setting(db, SETTING_SCMD_NOTICE) == "1" => {
            match api::register_chat_notice(&access_token, args).await {
                Ok(()) => {
                    let mut st = shared.lock().unwrap();
                    st.log(&format!("[공지] 등록: {}", args));
                }
                Err(e) => {
                    let mut st = shared.lock().unwrap();
                    st.log(&format!("[공지] 등록 실패: {e}"));
                }
            }
            true
        }
        "!카테고리" if db::get_setting(db, SETTING_SCMD_CATEGORY) == "1" => {
            match api::search_categories(&client_id, &client_secret, args).await {
                Ok(cats) if !cats.is_empty() => {
                    // 가장 유사한 카테고리: 정확 일치 → 포함 → 첫 번째
                    let best = cats.iter()
                        .find(|c| c.category_value == args)
                        .or_else(|| cats.iter().find(|c| c.category_value.contains(args)))
                        .unwrap_or(&cats[0]);

                    match api::update_live_setting(&access_token, None, Some(&best.category_id), None).await {
                        Ok(()) => {
                            {
                                let mut st = shared.lock().unwrap();
                                st.category = best.category_value.clone();
                                st.log(&format!("[카테고리] 변경: {}", best.category_value));
                            }
                            let msg = format!("카테고리가 \"{}\"(으)로 변경되었습니다.", best.category_value);
                            let _ = api::send_chat(&access_token, &msg).await;
                        }
                        Err(e) => {
                            let mut st = shared.lock().unwrap();
                            st.log(&format!("[카테고리] 변경 실패: {e}"));
                        }
                    }
                }
                Ok(_) => {
                    let _ = api::send_chat(&access_token, "해당 카테고리를 찾을 수 없습니다.").await;
                }
                Err(e) => {
                    let mut st = shared.lock().unwrap();
                    st.log(&format!("[카테고리] 검색 실패: {e}"));
                }
            }
            true
        }
        "!태그" if db::get_setting(db, SETTING_SCMD_TAG) == "1" => {
            // 현재 태그 조회
            let current_tags = match api::get_live_setting(&access_token).await {
                Ok(s) => s.tags.unwrap_or_default(),
                Err(_) => vec![],
            };

            let tag = args.to_string();
            let mut new_tags = current_tags.clone();
            let msg = if let Some(pos) = new_tags.iter().position(|t| t == &tag) {
                new_tags.remove(pos);
                format!("태그 \"{}\" 제거됨 (현재 {}개)", tag, new_tags.len())
            } else {
                if new_tags.len() >= 6 {
                    let _ = api::send_chat(&access_token, "태그는 최대 6개까지 가능합니다.").await;
                    return true;
                }
                new_tags.push(tag.clone());
                format!("태그 \"{}\" 추가됨 (현재 {}개)", tag, new_tags.len())
            };

            match api::update_live_setting(&access_token, None, None, Some(&new_tags)).await {
                Ok(()) => {
                    {
                        let mut st = shared.lock().unwrap();
                        st.log(&format!("[태그] {}", msg));
                    }
                    let _ = api::send_chat(&access_token, &msg).await;
                }
                Err(e) => {
                    let mut st = shared.lock().unwrap();
                    st.log(&format!("[태그] 변경 실패: {e}"));
                }
            }
            true
        }
        _ => false,
    }
}

/// 후원 이벤트 처리
pub async fn handle_donation(shared: &Shared, db: &Db, event: &DonationEvent) {
    let sender = event
        .donator_nickname
        .as_deref()
        .unwrap_or("익명");

    let sender_cid = event.donator_channel_id.as_deref();

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!(
            "[후원] {}님 {}원 후원",
            sender, event.pay_amount
        ));
    }

    let access_token = {
        let st = shared.lock().unwrap();
        st.access_token.clone().unwrap_or_default()
    };
    if access_token.is_empty() {
        return;
    }

    // 후원 메시지 규칙 매칭
    if let Some(rule) = db::find_donation_rule(db, event.pay_amount) {
        let template = rule.message.replace("<받은금액>", &event.pay_amount.to_string());
        let reply = replace_variables(&template, sender, shared, sender_cid).await;

        let reply = if reply.chars().count() > 100 {
            reply.chars().take(100).collect()
        } else {
            reply
        };

        match api::send_chat(&access_token, &reply).await {
            Ok(_) => {
                let mut st = shared.lock().unwrap();
                st.log(&format!("[후원 응답] {}", reply));
            }
            Err(e) => {
                let mut st = shared.lock().unwrap();
                st.log(&format!("[ERROR] 후원 응답 전송 실패: {e}"));
            }
        }
    }

}

/// 구독 이벤트 처리
pub async fn handle_subscription(shared: &Shared, db: &Db, event: &SubscriptionEvent) {
    let enabled = db::get_setting(db, SETTING_SUB_ENABLED);
    if enabled != "1" { return; }

    let sender = &event.subscriber_channel_name;
    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("[구독] {}님 {}개월 구독 (Tier {})", sender, event.month, event.tier_no));
    }

    let access_token = {
        let st = shared.lock().unwrap();
        st.access_token.clone().unwrap_or_default()
    };
    if access_token.is_empty() { return; }

    if let Some(rule) = db::find_subscription_rule(db, event.tier_no) {
        let template = rule.message
            .replace("<구독월>", &event.month.to_string())
            .replace("<구독메시지>", &event.subscription_message)
            .replace("<티어>", &event.tier_no.to_string());
        let reply = replace_variables(&template, sender, shared, Some(&event.subscriber_channel_id)).await;
        let reply = if reply.chars().count() > 100 { reply.chars().take(100).collect() } else { reply };

        match api::send_chat(&access_token, &reply).await {
            Ok(_) => {
                let mut st = shared.lock().unwrap();
                st.log(&format!("[구독 응답] {}", reply));
            }
            Err(e) => {
                let mut st = shared.lock().unwrap();
                st.log(&format!("[ERROR] 구독 응답 전송 실패: {e}"));
            }
        }
    }
}
