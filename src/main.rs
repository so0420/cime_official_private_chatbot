mod app;
mod auth;
mod api;
mod bot;
mod db;
mod gui;
mod sr;
mod ws;

use std::sync::Arc;
use crate::db::Db;
use crate::app::Shared;

async fn start_bot(shared: Shared, db: Db) {
    {
        let mut st = shared.lock().unwrap();
        st.bot_running = true;
        st.bot_should_stop = false;
        st.log("봇 자동 시작...");
    }
    // SR 대기열 복원
    sr::restore_queue(&shared, &db);
    let (s2, d2) = (shared.clone(), db.clone());
    tokio::spawn(async move { ws::event_loop(s2, d2).await; });
    let (s3, d3) = (shared.clone(), db.clone());
    tokio::spawn(async move { api::channel_info_loop(s3, d3).await; });
    let sr_port: u16 = db::get_setting(&db, app::SETTING_SR_PORT).parse().unwrap_or(8081);
    let (s4, d4) = (shared.clone(), db.clone());
    tokio::spawn(async move { sr::start_sr_server(s4, d4, sr_port).await; });
    let (s5, d5) = (shared, db);
    tokio::spawn(async move { bot::timer_loop(&s5, &d5).await; });
}

fn main() {
    env_logger::init();

    // DB 초기화
    let database = db::open_db();
    db::init_db(&database);

    // Tokio 런타임
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio 런타임 생성 실패"),
    );

    // 공유 상태
    let shared = app::new_shared();

    // 저장된 세션 복원 → 성공 시 봇 자동 시작
    {
        let shared2 = shared.clone();
        let db2 = database.clone();
        rt.spawn(async move {
            auth::restore_session(&shared2, &db2).await;
            // 로그인 성공 시 봇 자동 시작
            let logged_in = { shared2.lock().unwrap().logged_in };
            if logged_in {
                start_bot(shared2, db2).await;
            }
        });
    }

    // 토큰 자동 갱신 루프 (백그라운드)
    {
        let shared3 = shared.clone();
        let db3 = database.clone();
        rt.spawn(async move {
            auth::token_refresh_loop(shared3, db3).await;
        });
    }

    // GUI 실행
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([850.0, 680.0])
            .with_min_inner_size([650.0, 480.0]),
        ..Default::default()
    };

    let shared_gui = shared.clone();
    let db_gui = database.clone();
    let rt_gui = rt.clone();

    eframe::run_native(
        "CIME 챗봇",
        options,
        Box::new(move |cc| {
            Ok(Box::new(gui::BotGui::new(cc, shared_gui, db_gui, rt_gui)))
        }),
    )
    .expect("eframe 실행 실패");
}
