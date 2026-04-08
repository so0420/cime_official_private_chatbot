use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::app::*;

pub type Db = Arc<Mutex<Connection>>;

pub fn data_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new(".")).join("cime_bot");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn db_path() -> PathBuf {
    data_dir().join("bot.db")
}

pub fn open_db() -> Db {
    let conn = Connection::open(db_path()).expect("DB를 열 수 없습니다");
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .expect("PRAGMA 설정 실패");
    Arc::new(Mutex::new(conn))
}

pub fn init_db(db: &Db) {
    let conn = db.lock().unwrap();
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS commands (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            trigger_text  TEXT    UNIQUE NOT NULL,
            response      TEXT    NOT NULL,
            fail_response TEXT    DEFAULT '',
            is_attendance INTEGER DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS attendance (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id         TEXT NOT NULL,
            username        TEXT NOT NULL,
            attendance_date TEXT NOT NULL,
            created_at      TEXT DEFAULT (datetime('now', '+9 hours')),
            UNIQUE(user_id, attendance_date)
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS donation_rules (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            min_amount INTEGER NOT NULL DEFAULT 0,
            max_amount INTEGER NOT NULL DEFAULT 10000000,
            message    TEXT    NOT NULL,
            sort_order INTEGER DEFAULT 0
        );


        CREATE TABLE IF NOT EXISTS subscription_rules (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            tier_no    INTEGER NOT NULL,
            message    TEXT    NOT NULL,
            sort_order INTEGER DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sr_queue (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            video_id       TEXT    NOT NULL,
            video_title    TEXT    NOT NULL,
            video_duration INTEGER DEFAULT 0,
            requester      TEXT    NOT NULL,
            status         TEXT    DEFAULT 'queued',
            created_at     TEXT    DEFAULT (datetime('now', '+9 hours'))
        );

        CREATE TABLE IF NOT EXISTS timer_messages (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            name             TEXT    NOT NULL,
            message          TEXT    NOT NULL,
            interval_minutes INTEGER NOT NULL DEFAULT 5,
            enabled          INTEGER DEFAULT 1
        );
        ",
    )
    .expect("테이블 생성 실패");

    // 기본 설정
    let defaults = [
        (SETTING_ATTENDANCE_RESET_HOUR, "5"),
        (SETTING_CLIENT_ID, ""),
        (SETTING_CLIENT_SECRET, ""),
        (SETTING_ACCESS_TOKEN, ""),
        (SETTING_REFRESH_TOKEN, ""),
        (SETTING_CHANNEL_ID, ""),
        (SETTING_CHANNEL_NAME, ""),
        (SETTING_SCMD_TITLE, "1"),
        (SETTING_SCMD_NOTICE, "1"),
        (SETTING_SCMD_CATEGORY, "1"),
        (SETTING_SCMD_TAG, "1"),
        (SETTING_SUB_ENABLED, "1"),
        (SETTING_SR_ENABLED, "1"),
        (SETTING_SR_MAX_DURATION, "600"),
        (SETTING_SR_PORT, "8081"),
    ];
    for (k, v) in &defaults {
        conn.execute(
            "INSERT OR IGNORE INTO settings(key, value) VALUES(?1, ?2)",
            params![k, v],
        )
        .ok();
    }

    // 기본 명령어
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        let cmds = [
            ("!출첵", "<보낸사람>님 <출석횟수>번째 출석입니다.", "<보낸사람>님 이미 출석을 진행하였습니다.", 1),
            ("!방제", "<방제>", "", 0),
            ("!카테고리", "<카테고리>", "", 0),
            ("!업타임", "<업타임>동안 방송중", "", 0),
        ];
        for (trigger, resp, fail, att) in &cmds {
            conn.execute(
                "INSERT OR IGNORE INTO commands(trigger_text, response, fail_response, is_attendance) VALUES(?1,?2,?3,?4)",
                params![trigger, resp, fail, att],
            )
            .ok();
        }
    }

    // 기본 후원 규칙
    let dcount: i64 = conn
        .query_row("SELECT COUNT(*) FROM donation_rules", [], |r| r.get(0))
        .unwrap_or(0);
    if dcount == 0 {
        conn.execute(
            "INSERT INTO donation_rules(min_amount, max_amount, message, sort_order) VALUES(?1,?2,?3,?4)",
            params![0, 10000000, "<보낸사람>님 <받은금액>원 후원 감사합니다!", 0],
        )
        .ok();
    }

    let scount: i64 = conn.query_row("SELECT COUNT(*) FROM subscription_rules", [], |r| r.get(0)).unwrap_or(0);
    if scount == 0 {
        let sub_defaults = [
            (1, "<보낸사람>님 <구독월>개월 구독 감사합니다!"),
            (2, "<보낸사람>님 <구독월>개월 구독 감사합니다! (Tier 2)"),
        ];
        for (i, (tier, msg)) in sub_defaults.iter().enumerate() {
            conn.execute(
                "INSERT INTO subscription_rules(tier_no, message, sort_order) VALUES(?1,?2,?3)",
                params![tier, msg, i as i32],
            ).ok();
        }
    }
}

// ── Settings ──

pub fn get_setting(db: &Db, key: &str) -> String {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![key],
        |r| r.get(0),
    )
    .unwrap_or_default()
}

pub fn set_setting(db: &Db, key: &str, value: &str) {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO settings(key, value) VALUES(?1, ?2)",
        params![key, value],
    )
    .ok();
}

// ── Commands ──

pub fn list_commands(db: &Db) -> Vec<Command> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, trigger_text, response, fail_response, is_attendance FROM commands ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| {
        Ok(Command {
            id: r.get(0)?,
            trigger: r.get(1)?,
            response: r.get(2)?,
            fail_response: r.get::<_, String>(3).unwrap_or_default(),
            is_attendance: r.get::<_, i32>(4).unwrap_or(0) != 0,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn find_command(db: &Db, trigger: &str) -> Option<Command> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT id, trigger_text, response, fail_response, is_attendance FROM commands WHERE trigger_text=?1",
        params![trigger],
        |r| {
            Ok(Command {
                id: r.get(0)?,
                trigger: r.get(1)?,
                response: r.get(2)?,
                fail_response: r.get::<_, String>(3).unwrap_or_default(),
                is_attendance: r.get::<_, i32>(4).unwrap_or(0) != 0,
            })
        },
    )
    .ok()
}

pub fn add_command(db: &Db, trigger: &str, response: &str, fail_response: &str, is_attendance: bool) -> Result<(), String> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO commands(trigger_text, response, fail_response, is_attendance) VALUES(?1,?2,?3,?4)",
        params![trigger, response, fail_response, is_attendance as i32],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

pub fn update_command(db: &Db, id: i64, trigger: &str, response: &str, fail_response: &str, is_attendance: bool) {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE commands SET trigger_text=?1, response=?2, fail_response=?3, is_attendance=?4 WHERE id=?5",
        params![trigger, response, fail_response, is_attendance as i32, id],
    )
    .ok();
}

pub fn delete_command(db: &Db, id: i64) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM commands WHERE id=?1", params![id]).ok();
}

// ── Attendance ──

pub fn get_attendance_date(reset_hour: u32) -> String {
    let now = chrono::Local::now();
    let adjusted = if (now.hour()) < reset_hour {
        now - chrono::Duration::days(1)
    } else {
        now
    };
    adjusted.format("%Y-%m-%d").to_string()
}

use chrono::Timelike;

pub fn do_attendance(db: &Db, user_id: &str, username: &str, reset_hour: u32) -> (bool, i64) {
    let date = get_attendance_date(reset_hour);
    let conn = db.lock().unwrap();

    let already: bool = conn
        .query_row(
            "SELECT 1 FROM attendance WHERE user_id=?1 AND attendance_date=?2",
            params![user_id, date],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if already {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attendance WHERE user_id=?1",
                params![user_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        return (false, count);
    }

    conn.execute(
        "INSERT INTO attendance(user_id, username, attendance_date) VALUES(?1,?2,?3)",
        params![user_id, username, date],
    )
    .ok();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attendance WHERE user_id=?1",
            params![user_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    (true, count)
}

// ── Donation Rules ──

pub fn list_donation_rules(db: &Db) -> Vec<DonationRule> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, min_amount, max_amount, message, sort_order FROM donation_rules ORDER BY sort_order, id")
        .unwrap();
    stmt.query_map([], |r| {
        Ok(DonationRule {
            id: r.get(0)?,
            min_amount: r.get(1)?,
            max_amount: r.get(2)?,
            message: r.get(3)?,
            sort_order: r.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn find_donation_rule(db: &Db, amount: i64) -> Option<DonationRule> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT id, min_amount, max_amount, message, sort_order FROM donation_rules WHERE min_amount <= ?1 AND max_amount >= ?1 ORDER BY sort_order LIMIT 1",
        params![amount],
        |r| {
            Ok(DonationRule {
                id: r.get(0)?,
                min_amount: r.get(1)?,
                max_amount: r.get(2)?,
                message: r.get(3)?,
                sort_order: r.get(4)?,
            })
        },
    )
    .ok()
}

pub fn save_donation_rules(db: &Db, rules: &[DonationRule]) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM donation_rules", []).ok();
    for (i, rule) in rules.iter().enumerate() {
        conn.execute(
            "INSERT INTO donation_rules(min_amount, max_amount, message, sort_order) VALUES(?1,?2,?3,?4)",
            params![rule.min_amount, rule.max_amount, rule.message, i as i32],
        )
        .ok();
    }
}

// ── Subscription Rules ──

pub fn list_subscription_rules(db: &Db) -> Vec<crate::app::SubscriptionRule> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, tier_no, message FROM subscription_rules ORDER BY sort_order, id")
        .unwrap();
    stmt.query_map([], |r| {
        Ok(crate::app::SubscriptionRule {
            id: r.get(0)?,
            tier_no: r.get(1)?,
            message: r.get(2)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn find_subscription_rule(db: &Db, tier_no: i32) -> Option<crate::app::SubscriptionRule> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT id, tier_no, message FROM subscription_rules WHERE tier_no=?1",
        params![tier_no],
        |r| {
            Ok(crate::app::SubscriptionRule {
                id: r.get(0)?,
                tier_no: r.get(1)?,
                message: r.get(2)?,
            })
        },
    ).ok()
}

pub fn save_subscription_rules(db: &Db, rules: &[crate::app::SubscriptionRule]) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM subscription_rules", []).ok();
    for (i, rule) in rules.iter().enumerate() {
        conn.execute(
            "INSERT INTO subscription_rules(tier_no, message, sort_order) VALUES(?1,?2,?3)",
            params![rule.tier_no, rule.message, i as i32],
        ).ok();
    }
}

// ── SR Queue ──

pub fn sr_add(db: &Db, video_id: &str, title: &str, duration: i64, requester: &str) {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO sr_queue(video_id, video_title, video_duration, requester) VALUES(?1,?2,?3,?4)",
        params![video_id, title, duration, requester],
    ).ok();
}

pub fn sr_peek_next(db: &Db) -> Option<crate::app::SrQueueItem> {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT id, video_id, video_title, video_duration, requester, status FROM sr_queue WHERE status='queued' ORDER BY id LIMIT 1",
        [],
        |r| Ok(crate::app::SrQueueItem {
            id: r.get(0)?, video_id: r.get(1)?, video_title: r.get(2)?,
            video_duration: r.get(3)?, requester: r.get(4)?, status: r.get(5)?,
        }),
    ).ok()
}

pub fn sr_set_playing(db: &Db, id: i64) {
    let conn = db.lock().unwrap();
    conn.execute("UPDATE sr_queue SET status='playing' WHERE id=?1", params![id]).ok();
}

pub fn sr_remove_current(db: &Db) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM sr_queue WHERE status='playing'", []).ok();
}

pub fn sr_list(db: &Db, limit: i64) -> Vec<crate::app::SrQueueItem> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, video_id, video_title, video_duration, requester, status FROM sr_queue ORDER BY CASE status WHEN 'playing' THEN 0 ELSE 1 END, id LIMIT ?1"
    ).unwrap();
    stmt.query_map(params![limit], |r| Ok(crate::app::SrQueueItem {
        id: r.get(0)?, video_id: r.get(1)?, video_title: r.get(2)?,
        video_duration: r.get(3)?, requester: r.get(4)?, status: r.get(5)?,
    })).unwrap().filter_map(|r| r.ok()).collect()
}

pub fn sr_remove(db: &Db, id: i64) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM sr_queue WHERE id=?1", params![id]).ok();
}

pub fn sr_clear(db: &Db) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM sr_queue", []).ok();
}

pub fn sr_count_by_user(db: &Db, requester: &str) -> i64 {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM sr_queue WHERE requester=?1 AND status='queued'",
        params![requester], |r| r.get(0),
    ).unwrap_or(0)
}

pub fn sr_queue_count(db: &Db) -> i64 {
    let conn = db.lock().unwrap();
    conn.query_row("SELECT COUNT(*) FROM sr_queue WHERE status='queued'", [], |r| r.get(0)).unwrap_or(0)
}

// ── Timer Messages ──

pub fn list_timer_messages(db: &Db) -> Vec<crate::app::TimerMessage> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, name, message, interval_minutes, enabled FROM timer_messages ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| {
        Ok(crate::app::TimerMessage {
            id: r.get(0)?,
            name: r.get(1)?,
            message: r.get(2)?,
            interval_minutes: r.get(3)?,
            enabled: r.get::<_, i32>(4).unwrap_or(1) != 0,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn list_enabled_timer_messages(db: &Db) -> Vec<crate::app::TimerMessage> {
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, name, message, interval_minutes, enabled FROM timer_messages WHERE enabled=1 ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| {
        Ok(crate::app::TimerMessage {
            id: r.get(0)?,
            name: r.get(1)?,
            message: r.get(2)?,
            interval_minutes: r.get(3)?,
            enabled: true,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn add_timer_message(db: &Db, name: &str, message: &str, interval_minutes: i64) -> Result<(), String> {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO timer_messages(name, message, interval_minutes) VALUES(?1,?2,?3)",
        params![name, message, interval_minutes],
    )
    .map_err(|e| format!("{e}"))?;
    Ok(())
}

pub fn update_timer_message(db: &Db, id: i64, name: &str, message: &str, interval_minutes: i64, enabled: bool) {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE timer_messages SET name=?1, message=?2, interval_minutes=?3, enabled=?4 WHERE id=?5",
        params![name, message, interval_minutes, enabled as i32, id],
    )
    .ok();
}

pub fn set_timer_enabled(db: &Db, id: i64, enabled: bool) {
    let conn = db.lock().unwrap();
    conn.execute(
        "UPDATE timer_messages SET enabled=?1 WHERE id=?2",
        params![enabled as i32, id],
    )
    .ok();
}

pub fn delete_timer_message(db: &Db, id: i64) {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM timer_messages WHERE id=?1", params![id]).ok();
}
