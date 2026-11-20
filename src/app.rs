use crate::notification::{self, Action, WindowState};
use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use rust_auto_shutdown::{
    Mode, Timer, format_remaining, next_time, parse_minutes,
    schedule::{self, WeeklySchedule},
    shutdown,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const BACKGROUND: Color32 = Color32::from_rgb(242, 245, 250);
const INK: Color32 = Color32::from_rgb(26, 38, 61);
const MUTED: Color32 = Color32::from_rgb(97, 111, 133);
const BLUE: Color32 = Color32::from_rgb(47, 89, 210);
const LINE: Color32 = Color32::from_rgb(220, 227, 238);
const TINT: Color32 = Color32::from_rgb(234, 240, 255);
const NAVY: Color32 = Color32::from_rgb(29, 47, 82);
const RED: Color32 = Color32::from_rgb(177, 48, 65);
const GREEN: Color32 = Color32::from_rgb(34, 118, 89);
const WEEKLY_SUPPORTED: bool = cfg!(windows);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Weekly,
    Once,
}
#[derive(Clone, Copy, PartialEq)]
enum OnceMode {
    Minutes,
    Clock,
}
enum OnceSchedule {
    After(Duration),
    At(DateTime<Local>),
}
enum Pending {
    Once(OnceSchedule),
    Weekly(WeeklySchedule),
}

pub struct ShutdownApp {
    page: Page,
    once_mode: OnceMode,
    minutes: String,
    once_clock: String,
    once_simulation: bool,
    timer: Timer,
    active_mode: Mode,
    target: Option<DateTime<Local>>,
    weekly: WeeklySchedule,
    saved: Option<WeeklySchedule>,
    pending: Option<Pending>,
    notice: String,
    error: bool,
    last_refresh: Instant,
    last_run: String,
    reminder_window: WindowState,
    reminder_feedback: String,
    reminder_terminal: Option<notification::TerminalNotice>,
}

fn text(value: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(value).size(size).color(color)
}
fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .corner_radius(16)
        .inner_margin(24)
        .stroke(Stroke::new(1.0_f32, LINE))
}
fn button(ui: &mut egui::Ui, label: &str, selected: bool, size: Vec2) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(text(
            label,
            16.0,
            if selected { Color32::WHITE } else { INK },
        ))
        .fill(if selected { BLUE } else { Color32::WHITE })
        .stroke(Stroke::new(1.0_f32, if selected { BLUE } else { LINE }))
        .corner_radius(8),
    )
}
fn caption(ui: &mut egui::Ui, value: &str) {
    ui.label(text(value, 14.0, MUTED));
}

pub(crate) fn configure_theme(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());
    let mut fonts = egui::FontDefinitions::default();
    for (name, path) in platform_fonts() {
        if !fonts.font_data.contains_key(name)
            && let Ok(data) = std::fs::read(path)
        {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(data).into());
        }
    }
    for (family, candidates) in [
        (FontFamily::Proportional, ["body", "cjk"]),
        (FontFamily::Monospace, ["digits", "cjk"]),
    ] {
        let names = candidates
            .into_iter()
            .filter(|name| fonts.font_data.contains_key(*name))
            .map(str::to_owned);
        fonts
            .families
            .entry(family)
            .or_default()
            .splice(0..0, names);
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(16.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(14.0));
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 10.0);
    style.spacing.interact_size.y = 36.0;
    style.visuals.override_text_color = Some(INK);
    style.visuals.selection.bg_fill = BLUE;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    style.visuals.widgets.hovered.bg_fill = TINT;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BLUE);
    style.visuals.widgets.active.bg_fill = TINT;
    style.visuals.widgets.active.bg_stroke = Stroke::new(2.0_f32, BLUE);
    ctx.set_style(style);
}

#[cfg(not(target_os = "macos"))]
fn platform_fonts() -> Vec<(&'static str, PathBuf)> {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let directory = PathBuf::from(root).join("Fonts");
        return [
            ("body", "segoeui.ttf"),
            ("cjk", "msjh.ttc"),
            ("digits", "consola.ttf"),
        ]
        .map(|(name, file)| (name, directory.join(file)))
        .to_vec();
    }

    #[cfg(target_os = "linux")]
    if let Ok(result) = std::process::Command::new("fc-match")
        .args(["--format", "%{file}", ":lang=zh-tw"])
        .output()
        && result.status.success()
        && let Ok(path) = String::from_utf8(result.stdout)
        && !path.is_empty()
    {
        return vec![("cjk", PathBuf::from(path))];
    }

    Vec::new()
}

#[cfg(target_os = "macos")]
fn platform_fonts() -> Vec<(&'static str, PathBuf)> {
    [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ]
    .map(|path| ("cjk", PathBuf::from(path)))
    .to_vec()
}

impl ShutdownApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        configure_theme(ctx);
        let (saved, notice, error) = if WEEKLY_SUPPORTED {
            Self::load_weekly_status()
        } else {
            (None, "先用模擬模式確認時間，再啟用真正關機。".into(), false)
        };
        let weekly = saved.clone().unwrap_or_default();
        Self {
            page: if WEEKLY_SUPPORTED {
                Page::Weekly
            } else {
                Page::Once
            },
            once_mode: OnceMode::Minutes,
            minutes: "30".into(),
            once_clock: "23:30".into(),
            once_simulation: true,
            timer: Timer::default(),
            active_mode: Mode::Simulation,
            target: None,
            weekly,
            saved,
            pending: None,
            notice,
            error,
            last_refresh: Instant::now(),
            last_run: String::new(),
            reminder_window: WindowState::default(),
            reminder_feedback: String::new(),
            reminder_terminal: None,
        }
    }

    fn load_weekly_status() -> (Option<WeeklySchedule>, String, bool) {
        match schedule::load() {
            Ok(Some(value)) => match schedule::task_exists() {
                Ok(true) => {
                    let update = schedule::needs_reminder_update().and_then(|needed| {
                        if needed {
                            schedule::save(&value)?;
                        }
                        Ok(needed)
                    });
                    match update {
                        Ok(true) => (
                            Some(value),
                            "已更新每週排程，關機前 5 分鐘會顯示通知。".into(),
                            false,
                        ),
                        Ok(false) => (Some(value), "已載入 Windows 每週排程。".into(), false),
                        Err(error) => (
                            Some(value),
                            format!(
                                "提醒更新失敗，原排程可能仍使用舊版程式。請先關閉正在執行的倒數視窗，再重新儲存：{error}"
                            ),
                            true,
                        ),
                    }
                }
                Ok(false) => (None, "Windows 排程不存在，請重新儲存啟用。".into(), true),
                Err(error) => (None, error, true),
            },
            Ok(None) => (None, "先用模擬模式確認時間，再啟用真正關機。".into(), false),
            Err(error) => (None, error, true),
        }
    }

    fn report(&mut self, error: String) {
        self.notice = error;
        self.error = true;
    }
    fn save_weekly(&mut self, value: WeeklySchedule) {
        match schedule::save_from_ui(&value) {
            Ok(outcome) => {
                self.saved = Some(value);
                self.error = false;
                match outcome {
                    schedule::SaveOutcome::Scheduled => {
                        self.notice = "已儲存並啟用每週排程，關閉視窗後仍會生效。".into()
                    }
                    schedule::SaveOutcome::ReminderStarted => {
                        self.notice = "已儲存每週排程，正在開啟本次倒數通知。".into()
                    }
                    schedule::SaveOutcome::ReminderFailed(error) => {
                        self.report(format!("已儲存每週排程，但本次通知未啟動。{error}"))
                    }
                }
            }
            Err(error) => self.report(error),
        }
    }
    fn begin_once(&mut self, schedule: OnceSchedule) -> Result<(), String> {
        let now = Local::now();
        let duration = match schedule {
            OnceSchedule::After(d) => d,
            OnceSchedule::At(t) => t
                .signed_duration_since(now)
                .to_std()
                .map_err(|_| "指定時間已過，請重新設定。")?,
        };
        let mode = if self.once_simulation {
            Mode::Simulation
        } else {
            Mode::Shutdown
        };
        if duration < notification::MINIMUM_NOTICE {
            return Err("請至少預留 30 秒，讓關機通知有足夠的操作時間。".into());
        }
        self.timer.start(Instant::now(), duration, mode)?;
        self.reminder_window = WindowState::default();
        self.reminder_feedback.clear();
        self.reminder_terminal = None;
        self.target =
            Some(now + chrono::Duration::from_std(duration).map_err(|_| "時間超出範圍。")?);
        self.active_mode = mode;
        self.notice = if self.once_simulation {
            "模擬倒數中，到期只會顯示結果。"
        } else {
            "真正關機倒數中，請先儲存工作。"
        }
        .into();
        self.error = false;
        Ok(())
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 13, BLUE);
            let center = rect.center();
            let points: Vec<_> = (0..=32)
                .map(|i| {
                    let a = 0.65 + i as f32 / 32.0 * (std::f32::consts::TAU - 1.30);
                    center + egui::vec2(a.sin(), -a.cos()) * 12.0
                })
                .collect();
            p.add(egui::Shape::line(
                points,
                Stroke::new(2.5_f32, Color32::WHITE),
            ));
            p.line_segment(
                [
                    center + egui::vec2(0.0, -15.0),
                    center + egui::vec2(0.0, -2.0),
                ],
                Stroke::new(2.5_f32, Color32::WHITE),
            );
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(text(crate::APP_NAME, 25.0, INK).strong());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(text(format!("v{}", env!("CARGO_PKG_VERSION")), 13.0, MUTED));
            });
        });
        ui.add_space(10.0);
        if WEEKLY_SUPPORTED {
            ui.horizontal(|ui| {
                for (page, label) in [(Page::Weekly, "每週排程"), (Page::Once, "單次倒數")]
                {
                    if button(ui, label, self.page == page, egui::vec2(132.0, 42.0)).clicked() {
                        self.page = page;
                    }
                }
            });
            ui.add_space(10.0);
        }
        if self.timer.is_running() {
            egui::Frame::new()
                .fill(TINT)
                .inner_margin(10)
                .corner_radius(8)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mode = if self.active_mode == Mode::Simulation {
                            "模擬"
                        } else {
                            "真正關機"
                        };
                        ui.label(
                            text(
                                format!(
                                    "單次{mode}倒數中 · {}",
                                    format_remaining(
                                        self.timer.remaining(Instant::now()).unwrap_or_default()
                                    )
                                ),
                                16.0,
                                INK,
                            )
                            .strong(),
                        );
                        if ui.button("取消單次倒數").clicked() {
                            self.timer.cancel();
                            self.target = None;
                            self.notice = "已取消這次倒數。".into();
                            self.error = false;
                        }
                    });
                });
        }
    }

    fn time_input(ui: &mut egui::Ui, value: &mut String, label: &str) {
        ui.label(text(label, 15.0, MUTED));
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(BACKGROUND)
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 8))
                .stroke(Stroke::new(1.0_f32, LINE))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .font(FontId::monospace(38.0))
                            .desired_width(125.0)
                            .frame(false)
                            .char_limit(5)
                            .hint_text("23:30"),
                    );
                });
            ui.add_space(4.0);
            caption(ui, "24 小時制\nHH:MM");
        });
    }

    fn weekly_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(text("固定時間，重複執行", 20.0, INK).strong());
        caption(ui, "選擇時間與星期，建立每週關機排程。");
        ui.add_space(8.0);
        Self::time_input(ui, &mut self.weekly.time, "關機時間");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(text("重複星期", 15.0, MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("平日").clicked() {
                    self.weekly.days = [true, true, true, true, true, false, false];
                }
                if ui.small_button("每天").clicked() {
                    self.weekly.days = [true; 7];
                }
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let width = (ui.available_width() - 36.0) / 7.0;
            for (i, day) in ["一", "二", "三", "四", "五", "六", "日"]
                .iter()
                .enumerate()
            {
                if button(ui, day, self.weekly.days[i], egui::vec2(width, 44.0)).clicked() {
                    self.weekly.days[i] = !self.weekly.days[i];
                }
            }
        });
        ui.add_space(8.0);
        Self::simulation_control(ui, &mut self.weekly.simulation);
        ui.add_space(10.0);
        let valid = self.weekly.validate();
        if let Err(error) = &valid {
            ui.label(text(error, 14.0, RED));
        }
        if ui
            .add_enabled(
                valid.is_ok(),
                egui::Button::new(text("儲存並啟用排程", 16.0, Color32::WHITE))
                    .fill(BLUE)
                    .corner_radius(9)
                    .min_size(egui::vec2(ui.available_width(), 48.0)),
            )
            .clicked()
        {
            if self.weekly.simulation {
                self.save_weekly(self.weekly.clone());
            } else {
                self.pending = Some(Pending::Weekly(self.weekly.clone()));
            }
        }
        caption(ui, "關機前 5 分鐘通知；關閉主視窗後排程仍有效。");
    }

    fn simulation_control(ui: &mut egui::Ui, simulation: &mut bool) {
        egui::Frame::new()
            .fill(BACKGROUND)
            .inner_margin(12)
            .corner_radius(9)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.checkbox(simulation, text("模擬模式", 16.0, INK).strong());
                ui.label(text(
                    if *simulation {
                        "只記錄結果，不會關閉電腦。"
                    } else {
                        "到期會向作業系統要求關機，請先儲存工作。"
                    },
                    14.0,
                    if *simulation { MUTED } else { RED },
                ));
            });
    }

    fn once_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(text("只執行這一次", 20.0, INK).strong());
        caption(ui, "設定倒數，或指定下一個關機時刻。");
        let running = self.timer.is_running();
        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal(|ui| {
                if button(
                    ui,
                    "幾分鐘後",
                    self.once_mode == OnceMode::Minutes,
                    egui::vec2(112.0, 38.0),
                )
                .clicked()
                {
                    self.once_mode = OnceMode::Minutes;
                }
                if button(
                    ui,
                    "指定時刻",
                    self.once_mode == OnceMode::Clock,
                    egui::vec2(112.0, 38.0),
                )
                .clicked()
                {
                    self.once_mode = OnceMode::Clock;
                }
            });
            match self.once_mode {
                OnceMode::Minutes => {
                    ui.label(text("倒數分鐘", 15.0, MUTED));
                    egui::Frame::new()
                        .fill(BACKGROUND)
                        .corner_radius(10)
                        .inner_margin(egui::Margin::symmetric(16, 8))
                        .stroke(Stroke::new(1.0_f32, LINE))
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.minutes)
                                    .font(FontId::monospace(38.0))
                                    .desired_width(115.0)
                                    .frame(false)
                                    .char_limit(5),
                            );
                        });
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 24.0) / 4.0;
                        for value in [15, 30, 60, 120] {
                            if button(ui, &format!("{value} 分"), false, egui::vec2(width, 36.0))
                                .clicked()
                            {
                                self.minutes = value.to_string();
                            }
                        }
                    });
                }
                OnceMode::Clock => {
                    Self::time_input(ui, &mut self.once_clock, "執行時間");
                    caption(ui, "已過的時刻會排到明天。");
                }
            }
            ui.add_space(8.0);
            Self::simulation_control(ui, &mut self.once_simulation);
        });
        ui.add_space(12.0);
        if button(
            ui,
            if running {
                "取消這次倒數"
            } else {
                "開始倒數"
            },
            !running,
            egui::vec2(ui.available_width(), 48.0),
        )
        .clicked()
        {
            if running {
                self.timer.cancel();
                self.target = None;
                self.notice = "已取消這次倒數。".into();
                self.error = false;
            } else {
                let schedule = match self.once_mode {
                    OnceMode::Minutes => parse_minutes(&self.minutes).map(OnceSchedule::After),
                    OnceMode::Clock => {
                        next_time(&Local::now(), &self.once_clock).map(OnceSchedule::At)
                    }
                };
                match schedule {
                    Ok(schedule) if self.once_simulation => {
                        if let Err(error) = self.begin_once(schedule) {
                            self.report(error);
                        }
                    }
                    Ok(schedule) => self.pending = Some(Pending::Once(schedule)),
                    Err(error) => self.report(error),
                }
            }
        }
        caption(ui, "關閉視窗會取消單次倒數。");
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let weekly = self.page == Page::Weekly;
        let running = self.timer.is_running();
        let sim = if weekly {
            self.weekly.simulation
        } else if running {
            self.active_mode == Mode::Simulation
        } else {
            self.once_simulation
        };
        let preview_target = if weekly {
            self.weekly.next_occurrence(&Local::now()).ok()
        } else if running {
            self.target
        } else {
            match self.once_mode {
                OnceMode::Minutes => parse_minutes(&self.minutes)
                    .ok()
                    .and_then(|d| chrono::Duration::from_std(d).ok())
                    .map(|d| Local::now() + d),
                OnceMode::Clock => next_time(&Local::now(), &self.once_clock).ok(),
            }
        };
        egui::Frame::new()
            .fill(NAVY)
            .corner_radius(12)
            .inner_margin(22)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(text(
                    if !weekly && running {
                        "倒數進行中"
                    } else {
                        "下次執行預覽"
                    },
                    14.0,
                    Color32::from_rgb(188, 204, 232),
                ));
                let display = if !weekly && running {
                    format_remaining(self.timer.remaining(Instant::now()).unwrap_or_default())
                } else {
                    preview_target
                        .map(|t| t.format("%H:%M").to_string())
                        .unwrap_or("--:--".into())
                };
                ui.label(
                    text(
                        display,
                        if !weekly && running { 38.0 } else { 54.0 },
                        Color32::WHITE,
                    )
                    .monospace(),
                );
                ui.label(text(
                    preview_target
                        .map(|t| t.format("%Y/%m/%d").to_string())
                        .unwrap_or("請完成時間設定".into()),
                    15.0,
                    Color32::from_rgb(213, 224, 244),
                ));
                ui.add_space(12.0);
                ui.label(text(
                    if sim {
                        "預覽模式：模擬"
                    } else {
                        "預覽模式：真正關機"
                    },
                    14.0,
                    Color32::from_rgb(213, 224, 244),
                ));
            });
        ui.add_space(8.0);
        if weekly {
            ui.label(text("重複規則", 14.0, MUTED));
            ui.label(text(self.weekly.summary(), 16.0, INK));
            ui.separator();
            match &self.saved {
                Some(saved) => {
                    let dirty = *saved != self.weekly;
                    ui.label(
                        text(
                            if dirty {
                                "有尚未儲存的變更"
                            } else {
                                "排程已啟用"
                            },
                            15.0,
                            if dirty { MUTED } else { GREEN },
                        )
                        .strong(),
                    );
                    ui.label(text(format!("已生效：{}", saved.summary()), 14.0, MUTED));
                    ui.label(text(
                        if saved.simulation {
                            "目前生效模式：模擬"
                        } else {
                            "目前生效模式：真正關機"
                        },
                        14.0,
                        MUTED,
                    ));
                }
                None => {
                    ui.label(text("尚未啟用", 15.0, MUTED).strong());
                    caption(ui, "儲存後才會自動執行。");
                }
            }
            if self.saved.is_some()
                && button(
                    ui,
                    "停用每週排程",
                    false,
                    egui::vec2(ui.available_width(), 40.0),
                )
                .clicked()
            {
                match schedule::remove() {
                    Ok(()) => {
                        self.saved = None;
                        self.notice = "已停用每週排程。".into();
                        self.error = false;
                    }
                    Err(e) => self.report(e),
                }
            }
            if !self.last_run.is_empty() {
                caption(ui, &self.last_run);
            }
        } else {
            ui.label(text("單次倒數", 16.0, INK).strong());
            caption(ui, "僅執行一次，需保持視窗開啟。");
            if self.saved.is_some() {
                caption(ui, "另有每週排程啟用中；可切換頁面查看。");
            }
        }
    }

    fn confirmation(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("confirm_shutdown"))
            .frame(card())
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(text("確認啟用真正關機", 23.0, INK).strong());
                caption(ui, "請先儲存文件與工作。");
                match self.pending.as_ref() {
                    Some(Pending::Weekly(value)) => {
                        ui.label(text(value.summary(), 20.0, INK).strong());
                        ui.label("此排程會每週重複，關閉視窗後仍生效。");
                        if value.next_occurrence(&Local::now()).is_ok_and(|t| {
                            t.signed_duration_since(Local::now())
                                .to_std()
                                .is_ok_and(|d| d <= rust_auto_shutdown::REMINDER_LEAD)
                        }) {
                            caption(ui, "本次即將到期，確認後立即通知，至少預留 30 秒操作。");
                        }
                    }
                    Some(Pending::Once(OnceSchedule::After(d))) => {
                        ui.label(format!("確認後 {} 分鐘執行一次。", d.as_secs() / 60));
                    }
                    Some(Pending::Once(OnceSchedule::At(t))) => {
                        ui.label(t.format("%Y/%m/%d %H:%M").to_string());
                    }
                    None => {}
                }
                caption(ui, "不強制結束程式；未儲存文件可能阻擋關機。");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if button(ui, "返回設定", false, egui::vec2(140.0, 46.0)).clicked() {
                        self.pending = None;
                    }
                    if button(ui, "確認啟用", true, egui::vec2(260.0, 46.0)).clicked()
                        && let Some(value) = self.pending.take()
                    {
                        match value {
                            Pending::Weekly(v) => self.save_weekly(v),
                            Pending::Once(v) => {
                                if let Err(e) = self.begin_once(v) {
                                    self.report(e);
                                }
                            }
                        }
                    }
                });
            });
        if response.should_close() {
            self.pending = None;
        }
    }

    fn once_reminder(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if let Some(notice) = &self.reminder_terminal {
            let close = ctx.show_viewport_immediate(
                notification::viewport_id(),
                notification::builder(false),
                |child, _| notification::draw_terminal(child, notice),
            );
            if close {
                self.reminder_terminal = None;
            }
            return;
        }
        let Some(remaining) = self.timer.remaining(Instant::now()) else {
            self.reminder_window = WindowState::default();
            return;
        };
        let attention = self.reminder_window.observe(remaining);
        if !self.reminder_window.opened {
            return;
        }
        let target = self.target.unwrap_or_else(Local::now);
        let simulation = self.active_mode == Mode::Simulation;
        if attention && let Err(error) = self.reminder_window.announce(frame, target, simulation) {
            self.reminder_feedback = error;
        }
        let action = ctx.show_viewport_immediate(
            notification::viewport_id(),
            notification::builder(self.reminder_window.urgent),
            |child, class| {
                if class == egui::ViewportClass::Embedded {
                    // 無法提供獨立提醒時取消，避免沒有通知就繼續關機。
                    return Some(Action::Cancel);
                }
                if attention {
                    notification::request_attention(child);
                }
                notification::draw(
                    child,
                    remaining,
                    target,
                    simulation,
                    &self.reminder_feedback,
                )
            },
        );
        self.reminder_window.mark_shown(Instant::now());
        match action {
            Some(Action::Cancel) => {
                self.timer.cancel();
                self.target = None;
                self.reminder_window.dismiss_system_notice();
                self.notice = "已取消本次倒數。".into();
                self.error = false;
            }
            Some(Action::Extend(extra)) => match self.timer.extend(Instant::now(), extra) {
                Ok(remaining) => {
                    let target =
                        Local::now() + chrono::Duration::from_std(remaining).unwrap_or_default();
                    self.target = Some(target);
                    self.reminder_window.observe(remaining);
                    self.reminder_feedback = format!(
                        "已延後 {} 分鐘，本次預定 {}。",
                        extra.as_secs() / 60,
                        target.format("%H:%M")
                    );
                    self.notice.clone_from(&self.reminder_feedback);
                    self.error = false;
                    ctx.send_viewport_cmd_to(
                        notification::viewport_id(),
                        egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal),
                    );
                }
                Err(error) => self.reminder_feedback = error,
            },
            None => {}
        }
    }
}

impl eframe::App for ShutdownApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.timer.cancel();
            self.reminder_window.dismiss_system_notice();
            return;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(28))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.header(ui);
                    let width = ui.available_width();
                    let left = (width - 20.0) * 0.57;
                    let right = width - 20.0 - left;
                    ui.spacing_mut().item_spacing.x = 20.0;
                    ui.horizontal_top(|ui| {
                        card().show(ui, |ui| {
                            ui.set_width(left - 48.0);
                            ui.set_min_height(420.0);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                                ui.add_enabled_ui(self.pending.is_none(), |ui| {
                                    if self.page == Page::Weekly {
                                        self.weekly_editor(ui);
                                    } else {
                                        self.once_editor(ui);
                                    }
                                });
                            });
                        });
                        card().show(ui, |ui| {
                            ui.set_width(right - 48.0);
                            ui.set_min_height(420.0);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
                                self.preview(ui);
                            });
                        });
                    });
                    ui.add_space(12.0);
                    ui.label(text(
                        &self.notice,
                        14.0,
                        if self.error { RED } else { MUTED },
                    ));
                    if WEEKLY_SUPPORTED && self.page == Page::Weekly {
                        caption(
                            ui,
                            "每週排程需登入 Windows 且電腦保持喚醒；睡眠期間不會喚醒電腦。",
                        );
                    }
                });
            });
        self.confirmation(ctx);
        self.once_reminder(ctx, frame);
        let now = Instant::now();
        if self.timer.remaining(now) == Some(Duration::ZERO)
            && !self.reminder_window.can_execute(now)
        {
            self.timer.cancel();
            self.target = None;
            self.reminder_window.dismiss_system_notice();
            self.notice = "通知未能及時顯示，已取消本次關機。請重新設定倒數。".into();
            self.error = true;
            self.reminder_terminal = Some(notification::TerminalNotice::new(
                "本次已取消",
                &self.notice,
            ));
        } else if let Some(result) = self.timer.poll(now, shutdown::execute) {
            self.target = None;
            self.reminder_window.dismiss_system_notice();
            match result {
                Ok(Mode::Simulation) => {
                    self.notice = "模擬完成：倒數已到期，沒有執行關機。".into();
                    self.error = false;
                }
                Ok(Mode::Shutdown) => {
                    self.notice = "已送出關機要求。".into();
                    self.error = false;
                }
                Err(e) => {
                    self.reminder_terminal =
                        Some(notification::TerminalNotice::new("本次關機失敗", &e));
                    self.report(e);
                }
            }
            ctx.request_repaint();
        }
        if WEEKLY_SUPPORTED && self.last_refresh.elapsed() >= Duration::from_secs(3) {
            if let Ok(Some(record)) = schedule::last_run() {
                let time = DateTime::parse_from_rfc3339(&record.completed_at)
                    .map(|t| t.with_timezone(&Local).format("%m/%d %H:%M").to_string())
                    .unwrap_or_else(|_| "時間無法辨識".into());
                self.last_run = format!("最近執行 {time}\n{}", record.message);
            }
            self.last_refresh = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(if self.timer.is_running() {
            200
        } else {
            1000
        }));
    }
}
