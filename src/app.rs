use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

// ── 공유 상태 ──

pub type Shared = Arc<Mutex<SharedState>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(SharedState::default()))
}

#[derive(Debug)]
pub struct SharedState {
    // Auth
    pub client_id: String,
    pub client_secret: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub logged_in: bool,
    pub login_in_progress: bool,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,

    // Bot
    pub bot_running: bool,
    pub ws_connected: bool,
    pub bot_should_stop: bool,

    // Live info
    pub is_live: bool,
    pub live_title: String,
    pub category: String,
    pub opened_at: Option<String>,

    // Logs
    pub logs: VecDeque<String>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            access_token: None,
            refresh_token: None,
            token_expires_at: None,
            logged_in: false,
            login_in_progress: false,
            channel_id: None,
            channel_name: None,
            bot_running: false,
            ws_connected: false,
            bot_should_stop: false,
            is_live: false,
            live_title: String::new(),
            category: String::new(),
            opened_at: None,
            logs: VecDeque::with_capacity(500),
        }
    }
}

impl SharedState {
    pub fn log(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S");
        let line = format!("[{}] {}", ts, msg);
        if self.logs.len() >= 500 {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
}

// ── 명령어 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: i64,
    pub trigger: String,
    pub response: String,
    pub fail_response: String,
    pub is_attendance: bool,
}

// ── 후원 규칙 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DonationRule {
    pub id: i64,
    pub min_amount: i64,
    pub max_amount: i64,
    pub message: String,
    pub sort_order: i32,
}

// ── WebSocket 이벤트 ──

#[derive(Debug, Clone, Deserialize)]
pub struct WsEvent {
    pub event: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ChatEvent {
    pub channel_id: String,
    pub sender_channel_id: String,
    pub sender_nickname: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct DonationEvent {
    pub channel_id: String,
    pub donator_channel_id: Option<String>,
    pub donator_nickname: Option<String>,
    pub pay_amount: i64,
    pub donation_text: String,
}

// ── API 응답 ──

#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    pub code: Option<i32>,
    pub message: Option<String>,
    pub content: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub expires_in: i64,
    pub token_type: String,
}

/// "3600" 또는 3600 둘 다 i64로 파싱
fn deserialize_number_from_string<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct NumOrStr;
    impl<'de> de::Visitor<'de> for NumOrStr {
        type Value = i64;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a number or numeric string")
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<i64, E> { Ok(v) }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<i64, E> { Ok(v as i64) }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<i64, E> {
            v.parse().map_err(de::Error::custom)
        }
    }
    deserializer.deserialize_any(NumOrStr)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_handle: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub is_live: bool,
    pub title: Option<String>,
    pub opened_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSetting {
    pub default_live_title: Option<String>,
    pub category: Option<CategoryInfo>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInfo {
    pub category_id: Option<String>,
    pub category_type: Option<String>,
    pub category_value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAuth {
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub message_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowerInfo {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_handle: Option<String>,
    pub created_date: String,
}

// ── 구독 이벤트 ──

#[derive(Debug, Clone)]
pub struct SubscriptionEvent {
    pub channel_id: String,
    pub subscriber_channel_id: String,
    pub subscriber_channel_name: String,
    pub month: i32,
    pub tier_no: i32,
    pub subscription_message: String,
}

// ── 구독 규칙 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRule {
    pub id: i64,
    pub tier_no: i32,
    pub message: String,
}

// ── 카테고리 검색 결과 ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySearchResult {
    pub category_id: String,
    pub category_type: Option<String>,
    pub category_value: String,
}

// ── 설정 키 상수 ──

pub const SETTING_CLIENT_ID: &str = "client_id";
pub const SETTING_CLIENT_SECRET: &str = "client_secret";
pub const SETTING_ACCESS_TOKEN: &str = "access_token";
pub const SETTING_REFRESH_TOKEN: &str = "refresh_token";
pub const SETTING_CHANNEL_ID: &str = "channel_id";
pub const SETTING_CHANNEL_NAME: &str = "channel_name";
pub const SETTING_ATTENDANCE_RESET_HOUR: &str = "attendance_reset_hour";

// 스트리머 명령어
pub const SETTING_SCMD_TITLE: &str = "scmd_title";
pub const SETTING_SCMD_NOTICE: &str = "scmd_notice";
pub const SETTING_SCMD_CATEGORY: &str = "scmd_category";
pub const SETTING_SCMD_TAG: &str = "scmd_tag";

// 구독
pub const SETTING_SUB_ENABLED: &str = "subscription_enabled";

pub const BASE_URL: &str = "https://ci.me/api/openapi";
pub const AUTH_URL: &str = "https://ci.me/auth/openapi/account-interlock";
pub const REDIRECT_URI: &str = "http://localhost:3000/callback";
