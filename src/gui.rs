use eframe::egui;
use std::sync::Arc;

use crate::app::*;
use crate::db::{self, Db};

// ── 색상 팔레트 (웜 라이트 테마) ──
const ACCENT: egui::Color32 = egui::Color32::from_rgb(76, 175, 80);      // 초록 (메인 버튼)
const BROWN: egui::Color32 = egui::Color32::from_rgb(110, 76, 48);       // 갈색 (보조 버튼)
const GREEN: egui::Color32 = egui::Color32::from_rgb(76, 175, 80);
const RED: egui::Color32 = egui::Color32::from_rgb(220, 60, 60);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(200, 160, 40);
const DIM: egui::Color32 = egui::Color32::from_rgb(158, 142, 126);       // 연한 갈색 텍스트
const CARD_BG: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);   // 흰색 카드
const SIDEBAR_BG: egui::Color32 = egui::Color32::from_rgb(245, 230, 208);// 사이드바 베이지
const MAIN_BG: egui::Color32 = egui::Color32::from_rgb(253, 246, 235);   // 메인 크림색 배경
const TEXT_DARK: egui::Color32 = egui::Color32::from_rgb(74, 55, 40);    // 본문 텍스트 (진갈색)
const HEADING_COLOR: egui::Color32 = egui::Color32::from_rgb(93, 64, 55);// 제목 (초콜릿)
const TAG_BLUE: egui::Color32 = egui::Color32::from_rgb(56, 142, 60);
const TAG_AMBER: egui::Color32 = egui::Color32::from_rgb(180, 130, 40);
const INPUT_BORDER: egui::Color32 = egui::Color32::from_rgb(224, 213, 197);
const INPUT_BG: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Login,
    Commands,
    Donation,
    Subscription,
    Settings,
    Logs,
}

pub struct BotGui {
    shared: Shared,
    db: Db,
    rt: Arc<tokio::runtime::Runtime>,
    current_tab: Tab,

    // 로그인
    client_id_input: String,
    client_secret_input: String,

    // 명령어
    commands: Vec<Command>,
    cmd_trigger: String,
    cmd_response: String,
    cmd_fail_response: String,
    cmd_is_attendance: bool,
    editing_cmd_id: Option<i64>,
    commands_dirty: bool,

    // 후원
    donation_rules: Vec<DonationRule>,
    dr_min: String,
    dr_max: String,
    dr_msg: String,
    donation_dirty: bool,


    // 스트리머 명령어
    scmd_title: bool,
    scmd_notice: bool,
    scmd_category: bool,
    scmd_tag: bool,
    scmd_loaded: bool,

    // 구독
    sub_enabled: bool,
    sub_rules: Vec<SubscriptionRule>,
    sub_dirty: bool,
    sub_loaded: bool,
    sub_tier: String,
    sub_msg: String,

    // 설정
    attendance_reset_hour: String,
    settings_loaded: bool,

    // 로그
    log_auto_scroll: bool,
}

impl BotGui {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Shared,
        db: Db,
        rt: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        Self::setup_style(&cc.egui_ctx);
        Self::setup_korean_fonts(&cc.egui_ctx);

        let client_id_input = db::get_setting(&db, SETTING_CLIENT_ID);
        let client_secret_input = db::get_setting(&db, SETTING_CLIENT_SECRET);
        {
            let mut st = shared.lock().unwrap();
            if !client_id_input.is_empty() { st.client_id = client_id_input.clone(); }
            if !client_secret_input.is_empty() { st.client_secret = client_secret_input.clone(); }
        }

        Self {
            shared, db, rt,
            current_tab: Tab::Login,
            client_id_input, client_secret_input,
            commands: vec![], cmd_trigger: String::new(), cmd_response: String::new(),
            cmd_fail_response: String::new(), cmd_is_attendance: false, editing_cmd_id: None,
            commands_dirty: true,
            donation_rules: vec![], dr_min: "0".into(), dr_max: "10000000".into(),
            dr_msg: "<보낸사람>님 <받은금액>원 후원 감사합니다!".into(), donation_dirty: true,
            scmd_title: true, scmd_notice: true, scmd_category: true, scmd_tag: true, scmd_loaded: false,
            sub_enabled: true, sub_rules: vec![], sub_dirty: true, sub_loaded: false,
            sub_tier: "1".into(), sub_msg: String::new(),
            attendance_reset_hour: "5".into(), settings_loaded: false,
            log_auto_scroll: true,
        }
    }

    fn reload_commands(&mut self) { self.commands = db::list_commands(&self.db); self.commands_dirty = false; }
    fn reload_donation_rules(&mut self) { self.donation_rules = db::list_donation_rules(&self.db); self.donation_dirty = false; }
    fn reload_sub_rules(&mut self) { self.sub_rules = db::list_subscription_rules(&self.db); self.sub_dirty = false; }


    fn setup_style(ctx: &egui::Context) {
        ctx.set_visuals(egui::Visuals::light());
        let mut style = (*ctx.style()).clone();

        // 넉넉한 여백
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(16.0, 7.0);
        style.spacing.window_margin = egui::Margin::same(16);

        // 입력 필드 - 둥근 모서리 + 부드러운 테두리
        let input_round = egui::epaint::CornerRadius::same(8);
        style.visuals.widgets.noninteractive.corner_radius = input_round;
        style.visuals.widgets.inactive.corner_radius = input_round;
        style.visuals.widgets.hovered.corner_radius = input_round;
        style.visuals.widgets.active.corner_radius = input_round;
        style.visuals.widgets.open.corner_radius = input_round;
        style.visuals.widgets.inactive.bg_fill = INPUT_BG;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.2, egui::Color32::from_rgb(220, 210, 195));
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 252, 245);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(200, 180, 150));
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(255, 253, 248);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, GREEN);

        // 선택 색상
        style.visuals.selection.bg_fill = GREEN.linear_multiply(0.15);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, GREEN);

        // 텍스트
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_DARK);
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_DARK);

        // 배경
        style.visuals.panel_fill = MAIN_BG;
        style.visuals.window_fill = MAIN_BG;

        // 구분선을 부드럽게
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, egui::Color32::from_rgb(230, 220, 205));

        ctx.set_style(style);
    }

    fn setup_korean_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\malgun.ttf") {
            fonts.font_data.insert("malgun".into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
            if let Some(f) = fonts.families.get_mut(&egui::FontFamily::Proportional) { f.insert(0, "malgun".into()); }
            if let Some(f) = fonts.families.get_mut(&egui::FontFamily::Monospace) { f.push("malgun".into()); }
        }
        ctx.set_fonts(fonts);
    }
}

// ── 헬퍼: 카드 프레임 ──
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD_BG)
        .rounding(egui::Rounding::same(14))
        .inner_margin(egui::Margin::same(16))
        .stroke(egui::Stroke::new(0.8, egui::Color32::from_rgb(235, 225, 210)))
        .shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: egui::Color32::from_rgba_premultiplied(140, 120, 90, 18),
        })
        .show(ui, |ui| { add(ui); });
}

fn section_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).size(20.0).strong().color(HEADING_COLOR));
    ui.add_space(4.0);
}

fn sub_heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(14.0).strong().color(HEADING_COLOR));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(DIM));
}

fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
        .fill(GREEN)
        .rounding(egui::Rounding::same(10));
    ui.add(btn)
}

fn brown_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
        .fill(BROWN)
        .rounding(egui::Rounding::same(10));
    ui.add(btn)
}

fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
        .fill(RED.linear_multiply(0.85))
        .rounding(egui::Rounding::same(10));
    ui.add(btn)
}

fn tag(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let frame = egui::Frame::new()
        .fill(color.linear_multiply(0.12))
        .rounding(egui::Rounding::same(8))
        .inner_margin(egui::Margin::symmetric(8, 3));
    frame.show(ui, |ui| {
        ui.label(egui::RichText::new(text).size(11.0).color(color));
    });
}

fn labeled_field(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(egui::Color32::from_rgb(180, 180, 190)));
        ui.add(egui::TextEdit::singleline(value).desired_width(width));
    });
}

// ── App impl ──

impl eframe::App for BotGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        let (logged_in, bot_running, ws_connected, channel_name) = {
            let st = self.shared.lock().unwrap();
            (st.logged_in, st.bot_running, st.ws_connected, st.channel_name.clone())
        };

        // ── 사이드바 ──
        egui::SidePanel::left("sidebar").exact_width(160.0).frame(
            egui::Frame::new().fill(SIDEBAR_BG).inner_margin(egui::Margin::same(12))
        ).show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("CIME Bot").size(18.0).strong().color(HEADING_COLOR));
            ui.add_space(8.0);

            // 상태 카드
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(255, 250, 240))
                .rounding(egui::Rounding::same(12))
                .inner_margin(egui::Margin::same(10))
                .stroke(egui::Stroke::new(0.8, egui::Color32::from_rgb(235, 225, 210)))
                .show(ui, |ui| {
                    // 로그인 상태 (닉네임 포함)
                    ui.horizontal(|ui| {
                        let color = if logged_in { GREEN } else { RED };
                        ui.label(egui::RichText::new("●").size(10.0).color(color));
                        let label = if logged_in {
                            let name = channel_name.as_deref().unwrap_or("");
                            if name.is_empty() { "로그인됨".to_string() } else { format!("로그인됨({})", name) }
                        } else {
                            "미로그인".to_string()
                        };
                        ui.label(egui::RichText::new(label).size(12.0).color(color));
                    });
                    // 연결 상태
                    ui.horizontal(|ui| {
                        let (color, text) = if ws_connected {
                            (GREEN, "연결됨")
                        } else if bot_running {
                            (YELLOW, "연결 중...")
                        } else {
                            (DIM, "대기 중")
                        };
                        ui.label(egui::RichText::new("●").size(10.0).color(color));
                        ui.label(egui::RichText::new(text).size(12.0).color(color));
                    });
                });

            ui.add_space(12.0);

            // 탭 버튼
            let tabs = [
                (Tab::Login, "로그인"),
                (Tab::Commands, "명령어"),
                (Tab::Donation, "후원"),
                (Tab::Subscription, "구독"),
                (Tab::Settings, "설정"),
                (Tab::Logs, "로그"),
            ];
            for (tab, label) in &tabs {
                let selected = self.current_tab == *tab;
                let text_color = if selected { egui::Color32::WHITE } else { HEADING_COLOR };
                let bg = if selected { BROWN } else { egui::Color32::TRANSPARENT };

                let btn = egui::Button::new(
                    egui::RichText::new(*label).color(text_color).size(14.0)
                )
                .fill(bg)
                .rounding(egui::Rounding::same(10))
                .min_size(egui::vec2(136.0, 34.0));

                if ui.add(btn).clicked() {
                    self.current_tab = *tab;
                    match tab {
                        Tab::Commands => self.commands_dirty = true,
                        Tab::Donation => self.donation_dirty = true,
                        Tab::Subscription => self.sub_dirty = true,
                        _ => {}
                    }
                }
            }

            // 하단 봇 버튼
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if bot_running {
                    let btn = egui::Button::new(
                        egui::RichText::new("봇 중지").color(egui::Color32::WHITE).size(14.0)
                    ).fill(RED).rounding(egui::Rounding::same(10)).min_size(egui::vec2(136.0, 36.0));
                    if ui.add(btn).clicked() {
                        let mut st = self.shared.lock().unwrap();
                        st.bot_should_stop = true;
                        st.bot_running = false;
                        st.log("봇 중지 요청됨");
                    }
                } else if logged_in {
                    let btn = egui::Button::new(
                        egui::RichText::new("봇 시작").color(egui::Color32::WHITE).size(14.0)
                    ).fill(GREEN).rounding(egui::Rounding::same(10)).min_size(egui::vec2(136.0, 36.0));
                    if ui.add(btn).clicked() {
                        self.start_bot();
                    }
                }
            });
        });

        // ── 메인 패널 ──
        egui::CentralPanel::default().frame(
            egui::Frame::new().fill(MAIN_BG).inner_margin(egui::Margin::same(20))
        ).show(ctx, |ui| {
            match self.current_tab {
                Tab::Login => self.draw_login(ui),
                Tab::Commands => self.draw_commands(ui),
                Tab::Donation => self.draw_donation(ui),
                Tab::Subscription => self.draw_subscription(ui),
                Tab::Settings => self.draw_settings(ui),
                Tab::Logs => self.draw_logs(ui),
            }
        });
    }
}

impl BotGui {
    fn start_bot(&mut self) {
        let shared = self.shared.clone();
        let db = self.db.clone();
        { let mut st = shared.lock().unwrap(); st.bot_running = true; st.bot_should_stop = false; st.log("봇 시작..."); }
        let (s2, d2) = (shared.clone(), db.clone());
        self.rt.spawn(async move { crate::ws::event_loop(s2, d2).await; });
        let (s3, d3) = (shared, db);
        self.rt.spawn(async move { crate::api::channel_info_loop(s3, d3).await; });
    }

    // ── 로그인 ──
    fn draw_login(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, "로그인");

        let (logged_in, login_in_progress, channel_name) = {
            let st = self.shared.lock().unwrap();
            (st.logged_in, st.login_in_progress, st.channel_name.clone())
        };

        if logged_in {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("●").color(GREEN));
                    ui.label(egui::RichText::new(format!(
                        "로그인됨 ({})",
                        channel_name.as_deref().unwrap_or("알 수 없음")
                    )).color(GREEN).strong());
                });
            });
            ui.add_space(8.0);
        }

        card(ui, |ui| {
            sub_heading(ui, "API 인증 정보");
            ui.add_space(4.0);

            ui.label(egui::RichText::new("Client ID").color(DIM).size(12.0));
            ui.add(egui::TextEdit::singleline(&mut self.client_id_input).desired_width(f32::INFINITY));
            ui.add_space(4.0);

            ui.label(egui::RichText::new("Client Secret").color(DIM).size(12.0));
            ui.add(egui::TextEdit::singleline(&mut self.client_secret_input).password(true).desired_width(f32::INFINITY));
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let btn = egui::Button::new(
                    egui::RichText::new("로그인").color(egui::Color32::WHITE).size(14.0)
                ).fill(GREEN).rounding(egui::Rounding::same(10)).min_size(egui::vec2(110.0, 34.0));
                let btn = ui.add_enabled(!login_in_progress, btn);
                if btn.clicked() {
                    {
                        let mut st = self.shared.lock().unwrap();
                        st.client_id = self.client_id_input.clone();
                        st.client_secret = self.client_secret_input.clone();
                    }
                    db::set_setting(&self.db, SETTING_CLIENT_ID, &self.client_id_input);
                    db::set_setting(&self.db, SETTING_CLIENT_SECRET, &self.client_secret_input);
                    let shared = self.shared.clone();
                    let db = self.db.clone();
                    self.rt.spawn(async move {
                        if let Err(e) = crate::auth::login_flow(shared.clone(), db).await {
                            let mut st = shared.lock().unwrap();
                            st.login_in_progress = false;
                            st.log(&format!("로그인 실패: {e}"));
                        }
                    });
                }

                if login_in_progress {
                    ui.spinner();
                    ui.label(egui::RichText::new("브라우저에서 인증 중...").color(DIM));
                }
            });
        });

        ui.add_space(12.0);

        card(ui, |ui| {
            sub_heading(ui, "시작하기");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("1.");
                if ui.link(egui::RichText::new("ci.me 개발자 센터 열기").color(BROWN)).clicked() {
                    let _ = webbrowser::open("https://developers.ci.me/applications");
                }
            });
            hint(ui, "   애플리케이션을 생성하고 Client ID/Secret을 복사하세요.");
            ui.add_space(2.0);
            ui.label("2. Redirect URI 설정:");
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(248, 242, 232))
                    .rounding(egui::Rounding::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 5))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("http://localhost:3000/callback").monospace().size(12.0).color(HEADING_COLOR));
                    });
            });
            ui.add_space(2.0);
            ui.label("3. 위 필드에 입력 후 로그인 버튼을 누르세요.");
            ui.add_space(6.0);
            hint(ui, "필요 권한: READ:LIVE_CHAT, WRITE:LIVE_CHAT, READ:DONATION, READ:USER, READ:CHANNEL, READ:LIVE_STREAM_SETTINGS");
        });
    }

    // ── 명령어 ──
    fn draw_commands(&mut self, ui: &mut egui::Ui) {
        if self.commands_dirty { self.reload_commands(); }
        if !self.scmd_loaded {
            self.scmd_title = db::get_setting(&self.db, SETTING_SCMD_TITLE) == "1";
            self.scmd_notice = db::get_setting(&self.db, SETTING_SCMD_NOTICE) == "1";
            self.scmd_category = db::get_setting(&self.db, SETTING_SCMD_CATEGORY) == "1";
            self.scmd_tag = db::get_setting(&self.db, SETTING_SCMD_TAG) == "1";
            self.scmd_loaded = true;
        }

        section_heading(ui, "명령어 관리");

        // 스트리머 기본 명령어
        card(ui, |ui| {
            sub_heading(ui, "기본 명령어 (스트리머 전용)");
            hint(ui, "채널 소유자만 사용 가능합니다. 인자와 함께 입력하면 동작합니다.");
            ui.add_space(4.0);
            let cmds = [
                (&mut self.scmd_title, SETTING_SCMD_TITLE, "!방제 <새 방제>", "방송 제목 변경"),
                (&mut self.scmd_notice, SETTING_SCMD_NOTICE, "!공지 <내용>", "채팅 공지 등록"),
                (&mut self.scmd_category, SETTING_SCMD_CATEGORY, "!카테고리 <이름>", "카테고리 변경"),
                (&mut self.scmd_tag, SETTING_SCMD_TAG, "!태그 <태그>", "태그 추가/제거"),
            ];
            for (val, key, cmd, desc) in cmds {
                ui.horizontal(|ui| {
                    if ui.checkbox(val, "").changed() {
                        db::set_setting(&self.db, key, if *val { "1" } else { "0" });
                    }
                    ui.label(egui::RichText::new(cmd).strong().color(ACCENT));
                    ui.label(egui::RichText::new(desc).color(DIM).size(12.0));
                });
            }
        });

        ui.add_space(8.0);

        card(ui, |ui| {
            sub_heading(ui, if self.editing_cmd_id.is_some() { "명령어 수정" } else { "명령어 추가" });
            ui.add_space(4.0);

            ui.label(egui::RichText::new("트리거").color(DIM).size(12.0));
            ui.add(egui::TextEdit::singleline(&mut self.cmd_trigger).desired_width(f32::INFINITY).hint_text("예: !출첵"));
            ui.add_space(2.0);

            ui.label(egui::RichText::new("응답").color(DIM).size(12.0));
            ui.add(egui::TextEdit::multiline(&mut self.cmd_response).desired_width(f32::INFINITY).desired_rows(2).hint_text("예: <보낸사람>님 <출석횟수>번째 출석입니다."));

            ui.checkbox(&mut self.cmd_is_attendance, "출석체크 명령어");

            if self.cmd_is_attendance {
                ui.label(egui::RichText::new("출석 실패 시 응답").color(DIM).size(12.0));
                ui.add(egui::TextEdit::multiline(&mut self.cmd_fail_response).desired_width(f32::INFINITY).desired_rows(2).hint_text("예: <보낸사람>님 이미 출석하였습니다."));
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if self.editing_cmd_id.is_some() {
                    if accent_button(ui, "수정 완료").clicked() {
                        if let Some(id) = self.editing_cmd_id {
                            db::update_command(&self.db, id, &self.cmd_trigger, &self.cmd_response, &self.cmd_fail_response, self.cmd_is_attendance);
                            self.editing_cmd_id = None; self.clear_cmd_form(); self.commands_dirty = true;
                        }
                    }
                    if ui.button("취소").clicked() { self.editing_cmd_id = None; self.clear_cmd_form(); }
                } else {
                    if accent_button(ui, "추가").clicked() {
                        if !self.cmd_trigger.is_empty() && !self.cmd_response.is_empty() {
                            match db::add_command(&self.db, &self.cmd_trigger, &self.cmd_response, &self.cmd_fail_response, self.cmd_is_attendance) {
                                Ok(_) => { self.clear_cmd_form(); self.commands_dirty = true; }
                                Err(e) => { let mut st = self.shared.lock().unwrap(); st.log(&format!("명령어 추가 실패: {e}")); }
                            }
                        }
                    }
                }
            });
        });

        ui.add_space(4.0);
        hint(ui, "사용 가능 변수: <보낸사람>  <업타임>  <팔로우>  <출석횟수>  <방제>  <카테고리>");
        ui.add_space(8.0);

        // 명령어 목록
        egui::ScrollArea::vertical().show(ui, |ui| {
            let cmds = self.commands.clone();
            for cmd in &cmds {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&cmd.trigger).strong().size(14.0).color(BROWN));
                        if cmd.is_attendance {
                            tag(ui, "출석", TAG_BLUE);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(egui::RichText::new("삭제").size(11.0).color(RED)).clicked() {
                                db::delete_command(&self.db, cmd.id); self.commands_dirty = true;
                            }
                            if ui.small_button(egui::RichText::new("수정").size(11.0)).clicked() {
                                self.editing_cmd_id = Some(cmd.id);
                                self.cmd_trigger = cmd.trigger.clone();
                                self.cmd_response = cmd.response.clone();
                                self.cmd_fail_response = cmd.fail_response.clone();
                                self.cmd_is_attendance = cmd.is_attendance;
                            }
                        });
                    });
                    ui.label(&cmd.response);
                    if cmd.is_attendance && !cmd.fail_response.is_empty() {
                        ui.label(egui::RichText::new(format!("실패: {}", cmd.fail_response)).size(12.0).color(TAG_AMBER));
                    }
                });
                ui.add_space(2.0);
            }
        });
    }

    fn clear_cmd_form(&mut self) {
        self.cmd_trigger.clear(); self.cmd_response.clear(); self.cmd_fail_response.clear(); self.cmd_is_attendance = false;
    }

    // ── 후원 ──
    fn draw_donation(&mut self, ui: &mut egui::Ui) {
        if self.donation_dirty { self.reload_donation_rules(); }

        section_heading(ui, "후원 메시지");
        hint(ui, "후원 금액 범위별로 다른 응답을 설정할 수 있습니다.  변수: <보낸사람> <받은금액> <업타임> <방제> <카테고리>");
        ui.add_space(8.0);

        card(ui, |ui| {
            sub_heading(ui, "새 규칙 추가");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("금액 범위").color(DIM).size(12.0));
                ui.add(egui::TextEdit::singleline(&mut self.dr_min).desired_width(80.0).hint_text("최소"));
                ui.label("~");
                ui.add(egui::TextEdit::singleline(&mut self.dr_max).desired_width(80.0).hint_text("최대"));
                ui.label(egui::RichText::new("원").color(DIM));
            });
            ui.add_space(2.0);
            ui.label(egui::RichText::new("메시지").color(DIM).size(12.0));
            ui.add(egui::TextEdit::multiline(&mut self.dr_msg).desired_width(f32::INFINITY).desired_rows(2));
            ui.add_space(4.0);
            if accent_button(ui, "추가").clicked() {
                let min: i64 = self.dr_min.parse().unwrap_or(0);
                let max: i64 = self.dr_max.parse().unwrap_or(10000000);
                if !self.dr_msg.is_empty() {
                    let mut rules = self.donation_rules.clone();
                    rules.push(DonationRule { id: 0, min_amount: min, max_amount: max, message: self.dr_msg.clone(), sort_order: rules.len() as i32 });
                    db::save_donation_rules(&self.db, &rules);
                    self.donation_dirty = true; self.dr_msg.clear();
                }
            }
        });

        ui.add_space(8.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            let rules = self.donation_rules.clone();
            let mut to_delete: Option<usize> = None;
            for (i, rule) in rules.iter().enumerate() {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        tag(ui, &format!("{}원 ~ {}원", rule.min_amount, rule.max_amount), ACCENT);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(egui::RichText::new("삭제").size(11.0).color(RED)).clicked() {
                                to_delete = Some(i);
                            }
                        });
                    });
                    ui.label(&rule.message);
                });
                ui.add_space(2.0);
            }
            if let Some(idx) = to_delete {
                let mut rules = self.donation_rules.clone(); rules.remove(idx);
                db::save_donation_rules(&self.db, &rules); self.donation_dirty = true;
            }
        });
    }

    // ── 구독 ──
    fn draw_subscription(&mut self, ui: &mut egui::Ui) {
        if !self.sub_loaded {
            self.sub_enabled = db::get_setting(&self.db, SETTING_SUB_ENABLED) == "1";
            self.sub_loaded = true;
        }
        if self.sub_dirty { self.reload_sub_rules(); }

        section_heading(ui, "구독 알림");

        card(ui, |ui| {
            if ui.checkbox(&mut self.sub_enabled, "구독 알림 활성화").changed() {
                db::set_setting(&self.db, SETTING_SUB_ENABLED, if self.sub_enabled { "1" } else { "0" });
            }
        });

        ui.add_space(8.0);

        card(ui, |ui| {
            sub_heading(ui, "티어별 메시지 추가");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Tier").color(DIM).size(12.0));
                ui.add(egui::TextEdit::singleline(&mut self.sub_tier).desired_width(40.0));
                ui.label(egui::RichText::new("메시지").color(DIM).size(12.0));
                ui.add(egui::TextEdit::singleline(&mut self.sub_msg).desired_width(f32::INFINITY).hint_text("<보낸사람>님 <구독월>개월 구독 감사합니다!"));
            });
            ui.add_space(2.0);
            if accent_button(ui, "추가 / 수정").clicked() && !self.sub_msg.is_empty() {
                let tier: i32 = self.sub_tier.parse().unwrap_or(1);
                let mut rules = self.sub_rules.clone();
                if let Some(existing) = rules.iter_mut().find(|r| r.tier_no == tier) {
                    existing.message = self.sub_msg.clone();
                } else {
                    rules.push(SubscriptionRule { id: 0, tier_no: tier, message: self.sub_msg.clone() });
                }
                rules.sort_by_key(|r| r.tier_no);
                db::save_subscription_rules(&self.db, &rules);
                self.sub_dirty = true;
                self.sub_msg.clear();
            }
            hint(ui, "변수: <보낸사람> <구독월> <구독메시지> <티어> <업타임> <방제> <카테고리>");
        });

        ui.add_space(8.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            let rules = self.sub_rules.clone();
            let mut to_delete: Option<usize> = None;
            for (i, rule) in rules.iter().enumerate() {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        tag(ui, &format!("Tier {}", rule.tier_no), ACCENT);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(egui::RichText::new("삭제").size(11.0).color(RED)).clicked() {
                                to_delete = Some(i);
                            }
                        });
                    });
                    ui.label(&rule.message);
                });
                ui.add_space(2.0);
            }
            if let Some(idx) = to_delete {
                let mut rules = self.sub_rules.clone();
                rules.remove(idx);
                db::save_subscription_rules(&self.db, &rules);
                self.sub_dirty = true;
            }
        });
    }

    // ── 설정 ──
    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        if !self.settings_loaded {
            self.attendance_reset_hour = db::get_setting(&self.db, SETTING_ATTENDANCE_RESET_HOUR);
            if self.attendance_reset_hour.is_empty() { self.attendance_reset_hour = "5".into(); }
            self.settings_loaded = true;
        }

        section_heading(ui, "설정");

        card(ui, |ui| {
            sub_heading(ui, "출석체크 초기화 시각");
            hint(ui, "이 시각 이전의 출석은 전날로 계산됩니다.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.attendance_reset_hour).desired_width(40.0));
                ui.label("시");
                if accent_button(ui, "저장").clicked() {
                    let hour: u32 = self.attendance_reset_hour.parse().unwrap_or(5).min(23);
                    self.attendance_reset_hour = hour.to_string();
                    db::set_setting(&self.db, SETTING_ATTENDANCE_RESET_HOUR, &self.attendance_reset_hour);
                    let mut st = self.shared.lock().unwrap();
                    st.log(&format!("출석 초기화 시각: {}시로 설정됨", hour));
                }
            });
        });

        ui.add_space(8.0);

        card(ui, |ui| {
            sub_heading(ui, "채널 정보");
            ui.add_space(4.0);
            let (channel_id, channel_name, is_live, title, category) = {
                let st = self.shared.lock().unwrap();
                (st.channel_id.clone().unwrap_or_default(), st.channel_name.clone().unwrap_or_default(),
                 st.is_live, st.live_title.clone(), st.category.clone())
            };

            egui::Grid::new("channel_info").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label(egui::RichText::new("채널 ID").color(DIM));
                ui.label(&channel_id);
                ui.end_row();
                ui.label(egui::RichText::new("채널 이름").color(DIM));
                ui.label(&channel_name);
                ui.end_row();
                ui.label(egui::RichText::new("방송 상태").color(DIM));
                let (color, text) = if is_live { (GREEN, "방송 중") } else { (RED, "오프라인") };
                ui.label(egui::RichText::new(text).color(color));
                ui.end_row();
                ui.label(egui::RichText::new("방제").color(DIM));
                ui.label(&title);
                ui.end_row();
                ui.label(egui::RichText::new("카테고리").color(DIM));
                ui.label(&category);
                ui.end_row();
            });
        });
    }

    // ── 로그 ──
    fn draw_logs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            section_heading(ui, "로그");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if danger_button(ui, "지우기").clicked() {
                    let mut st = self.shared.lock().unwrap(); st.logs.clear();
                }
                ui.checkbox(&mut self.log_auto_scroll, "자동 스크롤");
            });
        });

        let logs: Vec<String> = { let st = self.shared.lock().unwrap(); st.logs.iter().cloned().collect() };

        egui::Frame::new()
            .fill(egui::Color32::from_rgb(255, 253, 248))
            .rounding(egui::Rounding::same(14))
            .inner_margin(egui::Margin::same(14))
            .stroke(egui::Stroke::new(0.8, egui::Color32::from_rgb(235, 225, 210)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().stick_to_bottom(self.log_auto_scroll).show(ui, |ui| {
                    for line in &logs {
                        ui.label(egui::RichText::new(line).monospace().size(12.0).color(TEXT_DARK));
                    }
                });
            });
    }
}
