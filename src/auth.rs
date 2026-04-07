use std::sync::Arc;
use tokio::sync::oneshot;

use crate::app::*;
use crate::db::{self, Db};

/// OAuth 로그인 플로우
pub async fn login_flow(shared: Shared, db: Db) -> Result<(), String> {
    let (client_id, client_secret) = {
        let st = shared.lock().unwrap();
        (st.client_id.clone(), st.client_secret.clone())
    };

    if client_id.is_empty() || client_secret.is_empty() {
        return Err("Client ID와 Client Secret을 입력하세요.".into());
    }

    {
        let mut st = shared.lock().unwrap();
        st.login_in_progress = true;
        st.log("로그인 시작...");
    }

    let state_param = uuid::Uuid::new_v4().to_string();

    let auth_url = format!(
        "{}?clientId={}&redirectUri={}&state={}",
        AUTH_URL, &client_id, REDIRECT_URI, &state_param,
    );

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("인증 URL: {}", auth_url));
    }

    // 로컬 서버 바인딩
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .map_err(|e| format!("포트 3000 바인딩 실패: {e}"))?;

    {
        let mut st = shared.lock().unwrap();
        st.log("로컬 서버 시작 (127.0.0.1:3000)");
    }

    // code 수신 채널
    let (tx, rx) = oneshot::channel::<String>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

    let expected_state = state_param.clone();
    let shared_for_server = shared.clone();

    // 서버 태스크: code를 받을 때까지 여러 연결을 accept
    let server_task = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                let mut st = shared_for_server.lock().unwrap();
                st.log("[콜백] 타임아웃");
                break;
            }
            match tokio::time::timeout(remaining, listener.accept()).await {
                Ok(Ok((stream, addr))) => {
                    {
                        let mut st = shared_for_server.lock().unwrap();
                        st.log(&format!("[콜백] 연결 수신: {}", addr));
                    }
                    let got_code = handle_callback(stream, &tx, &expected_state, &shared_for_server).await;
                    if got_code {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    let mut st = shared_for_server.lock().unwrap();
                    st.log(&format!("[콜백] accept 오류: {e}"));
                    break;
                }
                Err(_) => {
                    let mut st = shared_for_server.lock().unwrap();
                    st.log("[콜백] 타임아웃");
                    break;
                }
            }
        }
    });

    // 브라우저 열기
    webbrowser::open(&auth_url).map_err(|e| format!("브라우저 열기 실패: {e}"))?;
    {
        let mut st = shared.lock().unwrap();
        st.log("브라우저에서 인증을 완료해주세요...");
    }

    // code 수신 대기
    let code = tokio::time::timeout(std::time::Duration::from_secs(120), rx)
        .await
        .map_err(|_| "인증 시간 초과 (120초)".to_string())?
        .map_err(|_| "인증 코드를 수신하지 못했습니다.".to_string())?;

    server_task.abort();

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("인증 코드 수신: {}...", &code[..code.len().min(10)]));
    }

    // 토큰 교환
    let token = exchange_code(&client_id, &client_secret, &code, &shared).await?;

    // 저장
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(token.expires_in);
    {
        let mut st = shared.lock().unwrap();
        st.access_token = Some(token.access_token.clone());
        st.refresh_token = Some(token.refresh_token.clone());
        st.token_expires_at = Some(expires_at);
        st.logged_in = true;
        st.login_in_progress = false;
        st.log("로그인 성공!");
    }

    db::set_setting(&db, SETTING_ACCESS_TOKEN, &token.access_token);
    db::set_setting(&db, SETTING_REFRESH_TOKEN, &token.refresh_token);

    // 사용자 정보 조회
    match fetch_user_info(&token.access_token).await {
        Ok(user) => {
            let mut st = shared.lock().unwrap();
            st.channel_id = Some(user.channel_id.clone());
            st.channel_name = Some(user.channel_name.clone());
            st.log(&format!("채널: {} ({})", user.channel_name, user.channel_id));
            drop(st);
            db::set_setting(&db, SETTING_CHANNEL_ID, &user.channel_id);
            db::set_setting(&db, SETTING_CHANNEL_NAME, &user.channel_name);
        }
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("사용자 정보 조회 실패: {e}"));
        }
    }

    Ok(())
}

/// 콜백 처리. code를 성공적으로 받았으면 true 반환.
async fn handle_callback(
    stream: tokio::net::TcpStream,
    tx: &Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
    expected_state: &str,
    shared: &Shared,
) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 4096];
    let mut stream = stream;
    let n = match stream.read(&mut buf).await {
        Ok(n) => n,
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[콜백] 읽기 오류: {e}"));
            return false;
        }
    };
    let request = String::from_utf8_lossy(&buf[..n]);

    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("[콜백] 요청: {}", first_line));
    }

    // /callback 경로가 아니면 무시 (favicon 등)
    if !path.starts_with("/callback") {
        let body = "OK";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return false;
    }

    let parsed = url::Url::parse(&format!("http://localhost{}", path));
    let url = match parsed {
        Ok(u) => u,
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[콜백] URL 파싱 실패: {e}, path: {path}"));
            return false;
        }
    };

    let code = url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.to_string());
    let state = url.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.to_string());

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!(
            "[콜백] code={}, state={}",
            code.as_deref().unwrap_or("없음"),
            state.as_deref().unwrap_or("없음")
        ));
    }

    if let Some(code) = code {
        // state 비교: 없거나 일치하면 OK
        let state_ok = state.is_none() || state.as_deref() == Some(expected_state);
        if state_ok {
            let body = "<html><head><meta charset='utf-8'></head><body><h1>인증 완료!</h1><p>이 창을 닫아도 됩니다.</p><script>window.close()</script></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;

            if let Some(tx) = tx.lock().await.take() {
                let _ = tx.send(code);
            }
            return true;
        } else {
            let mut st = shared.lock().unwrap();
            st.log(&format!(
                "[콜백] state 불일치: 예상={}, 수신={}",
                expected_state,
                state.as_deref().unwrap_or("없음")
            ));
        }
    }

    let body = "<html><head><meta charset='utf-8'></head><body><h1>인증 실패</h1></body></html>";
    let response = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    false
}

/// 인증 코드 → 토큰 교환
async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    shared: &Shared,
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/auth/v1/token", BASE_URL);

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("토큰 교환 요청: {}", url));
    }

    // JSON 방식 시도
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "grantType": "authorization_code",
            "clientId": client_id,
            "clientSecret": client_secret,
            "code": code,
        }))
        .send()
        .await
        .map_err(|e| format!("토큰 교환 요청 실패: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("토큰 교환 응답 ({}): {}", status, &body[..body.len().min(200)]));
    }

    if !status.is_success() {
        // JSON 실패 시 form-encoded 방식 재시도
        {
            let mut st = shared.lock().unwrap();
            st.log("JSON 방식 실패, form-encoded 방식으로 재시도...");
        }

        let resp2 = client
            .post(&url)
            .form(&[
                ("grantType", "authorization_code"),
                ("clientId", client_id),
                ("clientSecret", client_secret),
                ("code", code),
            ])
            .send()
            .await
            .map_err(|e| format!("토큰 교환 요청 실패 (form): {e}"))?;

        let status2 = resp2.status();
        let body2 = resp2.text().await.unwrap_or_default();

        {
            let mut st = shared.lock().unwrap();
            st.log(&format!("토큰 교환 응답 form ({}): {}", status2, &body2[..body2.len().min(200)]));
        }

        if !status2.is_success() {
            return Err(format!("토큰 교환 실패 ({}): {}", status2, body2));
        }

        return parse_token_response(&body2);
    }

    parse_token_response(&body)
}

fn parse_token_response(body: &str) -> Result<TokenResponse, String> {
    // 먼저 ApiResponse<TokenResponse> 형식 시도
    if let Ok(api_resp) = serde_json::from_str::<ApiResponse<TokenResponse>>(body) {
        if let Some(content) = api_resp.content {
            return Ok(content);
        }
    }

    // 바로 TokenResponse 형식 시도
    if let Ok(token) = serde_json::from_str::<TokenResponse>(body) {
        return Ok(token);
    }

    Err(format!("토큰 응답 파싱 실패: {}", &body[..body.len().min(300)]))
}

/// 토큰 갱신
pub async fn refresh_token(shared: &Shared, db: &Db) -> Result<(), String> {
    let (client_id, client_secret, refresh) = {
        let st = shared.lock().unwrap();
        (
            st.client_id.clone(),
            st.client_secret.clone(),
            st.refresh_token.clone().unwrap_or_default(),
        )
    };

    if refresh.is_empty() {
        return Err("Refresh token이 없습니다.".into());
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/v1/token", BASE_URL))
        .json(&serde_json::json!({
            "grantType": "refresh_token",
            "clientId": client_id,
            "clientSecret": client_secret,
            "refreshToken": refresh,
        }))
        .send()
        .await
        .map_err(|e| format!("토큰 갱신 요청 실패: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("토큰 갱신 실패 ({}): {}", status, body));
    }

    let token = parse_token_response(&body)?;
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(token.expires_in);

    {
        let mut st = shared.lock().unwrap();
        st.access_token = Some(token.access_token.clone());
        st.refresh_token = Some(token.refresh_token.clone());
        st.token_expires_at = Some(expires_at);
        st.logged_in = true;
        st.log("토큰 갱신 완료");
    }

    db::set_setting(db, SETTING_ACCESS_TOKEN, &token.access_token);
    db::set_setting(db, SETTING_REFRESH_TOKEN, &token.refresh_token);

    Ok(())
}

/// 토큰 자동 갱신 루프 (만료 5분 전에 갱신)
pub async fn token_refresh_loop(shared: Shared, db: Db) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;

        let should_refresh = {
            let st = shared.lock().unwrap();
            if !st.logged_in {
                continue;
            }
            match st.token_expires_at {
                Some(exp) => chrono::Utc::now() + chrono::Duration::minutes(5) >= exp,
                None => false,
            }
        };

        if should_refresh {
            if let Err(e) = refresh_token(&shared, &db).await {
                let mut st = shared.lock().unwrap();
                st.log(&format!("토큰 갱신 실패: {e}"));
            }
        }
    }
}

/// 사용자 정보 조회
async fn fetch_user_info(access_token: &str) -> Result<UserInfo, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/open/v1/users/me", BASE_URL))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("사용자 정보 조회 실패 ({}): {}", status, body));
    }

    let api_resp: ApiResponse<UserInfo> =
        serde_json::from_str(&body).map_err(|e| format!("파싱 실패: {e}, body: {body}"))?;

    api_resp.content.ok_or_else(|| format!("사용자 정보 없음: {body}"))
}

/// 저장된 토큰으로 자동 복원
pub async fn restore_session(shared: &Shared, db: &Db) {
    let access = db::get_setting(db, SETTING_ACCESS_TOKEN);
    let refresh = db::get_setting(db, SETTING_REFRESH_TOKEN);
    let client_id = db::get_setting(db, SETTING_CLIENT_ID);
    let client_secret = db::get_setting(db, SETTING_CLIENT_SECRET);
    let channel_id = db::get_setting(db, SETTING_CHANNEL_ID);
    let channel_name = db::get_setting(db, SETTING_CHANNEL_NAME);

    if access.is_empty() || refresh.is_empty() {
        return;
    }

    {
        let mut st = shared.lock().unwrap();
        st.client_id = client_id;
        st.client_secret = client_secret;
        st.refresh_token = Some(refresh);
        if !channel_id.is_empty() {
            st.channel_id = Some(channel_id);
        }
        if !channel_name.is_empty() {
            st.channel_name = Some(channel_name);
        }
    }

    match refresh_token(shared, db).await {
        Ok(()) => {
            let mut st = shared.lock().unwrap();
            st.log("저장된 세션 복원 완료");
        }
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("세션 복원 실패: {e}"));
            st.logged_in = false;
        }
    }
}
