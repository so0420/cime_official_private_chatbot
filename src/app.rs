use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
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

    // SR (노래신청)
    pub sr_queue_changed: bool,
    pub sr_command: Option<String>,
    pub sr_current_video_id: Option<String>,
    pub sr_current_title: Option<String>,
    pub sr_current_requester: Option<String>,
    pub sr_volume: i32,

    // 채널 관리자
    pub manager_channel_ids: HashSet<String>,

    // Logs
    pub logs: VecDeque<String>,
    pub log_file: Option<std::fs::File>,
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
            manager_channel_ids: HashSet::new(),
            sr_queue_changed: false,
            sr_command: None,
            sr_current_video_id: None,
            sr_current_title: None,
            sr_current_requester: None,
            sr_volume: 50,
            logs: VecDeque::with_capacity(500),
            log_file: None,
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
        self.logs.push_back(line.clone());
        if let Some(f) = &mut self.log_file {
            let _ = writeln!(f, "{}", line);
            let _ = f.flush();
        }
    }
}

/// 로그 디렉토리 및 파일 초기화.
/// logs/ 폴더 안에 날짜별 로그 파일 생성, 최신 2개만 유지.
pub fn init_log_file() -> Option<std::fs::File> {
    let logs_dir = log_dir();
    let _ = std::fs::create_dir_all(&logs_dir);

    // 기존 로그 파일 정리 (최신 2개만 유지)
    cleanup_old_logs(&logs_dir, 2);

    let filename = format!("{}.log", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let path = logs_dir.join(filename);
    std::fs::File::create(path).ok()
}

fn log_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new(".")).join("cime_bot").join("logs");
    dir
}

fn cleanup_old_logs(dir: &std::path::Path, keep: usize) {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "log").unwrap_or(false))
        .collect();

    if files.len() < keep {
        return;
    }

    // 수정 시간 기준 내림차순 정렬
    files.sort_by(|a, b| {
        let ta = a.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let tb = b.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        tb.cmp(&ta)
    });

    // keep개 이후 삭제
    for old in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(old.path());
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
    pub sender_slug: Option<String>,
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

// ── 채널 관리자 ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingRole {
    pub manager_channel_id: String,
    pub manager_channel_name: String,
    pub user_role: String,
}

// ── 반복 메세지 (타이머) ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerMessage {
    pub id: i64,
    pub name: String,
    pub message: String,
    pub interval_minutes: i64,
    pub enabled: bool,
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

// SR
pub const SETTING_SR_ENABLED: &str = "sr_enabled";
pub const SETTING_SR_MAX_DURATION: &str = "sr_max_duration";
pub const SETTING_SR_PORT: &str = "sr_port";

#[derive(Debug, Clone)]
pub struct SrQueueItem {
    pub id: i64,
    pub video_id: String,
    pub video_title: String,
    pub video_duration: i64,
    pub requester: String,
    pub status: String,
}

pub const BASE_URL: &str = "https://ci.me/api/openapi";
pub const AUTH_URL: &str = "https://ci.me/auth/openapi/account-interlock";
pub const REDIRECT_URI: &str = "http://localhost:3000/callback";
