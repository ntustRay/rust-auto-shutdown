use chrono::{DateTime, Local};
use eframe::egui::{self, Color32, RichText, Stroke};
use rust_auto_shutdown::{
    Mode, REMINDER_LEAD, Timer, format_remaining, schedule::ScheduledReminder, shutdown,
};
use std::time::{Duration, Instant};

const BLUE: Color32 = Color32::from_rgb(47, 89, 210);
const INK: Color32 = Color32::from_rgb(26, 38, 61);
const MUTED: Color32 = Color32::from_rgb(97, 111, 133);
const BACKGROUND: Color32 = Color32::from_rgb(242, 245, 250);
const LINE: Color32 = Color32::from_rgb(220, 227, 238);
const NAVY: Color32 = Color32::from_rgb(29, 47, 82);
pub const MINIMUM_NOTICE: Duration = Duration::from_secs(30);

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("shutdown-notice")
}

#[derive(Clone, Copy)]
pub enum Action {
    Extend(Duration),
    Cancel,
}

/// 延後後保持普通視窗，進入最後五分鐘時才再次置頂。
#[derive(Default)]
pub struct WindowState {
    pub opened: bool,
    pub urgent: bool,
    first_shown: Option<Instant>,
}

impl WindowState {
    pub fn observe(&mut self, remaining: Duration) -> bool {
        let urgent = remaining <= REMINDER_LEAD;
        let entered = urgent && !self.urgent;
        self.opened |= urgent;
        self.urgent = urgent;
        entered
    }

    pub fn mark_shown(&mut self, now: Instant) {
        self.first_shown.get_or_insert(now);
    }

    pub fn can_execute(&self, now: Instant) -> bool {
        self.first_shown
            .is_some_and(|shown| now.saturating_duration_since(shown) >= MINIMUM_NOTICE)
    }
}

pub struct TerminalNotice {
    pub title: String,
    pub message: String,
}

impl TerminalNotice {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
        }
    }
}

pub fn draw_terminal(ctx: &egui::Context, notice: &TerminalNotice) -> bool {
    if ctx.input(|i| i.viewport().close_requested()) {
        return true;
    }
    let mut close = false;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(24))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new(&notice.title).size(25.0).strong().color(INK));
                ui.add_space(12.0);
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(18)
                    .corner_radius(12)
                    .stroke(Stroke::new(1.0_f32, LINE))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&notice.message).size(16.0).color(INK));
                    });
                ui.add_space(12.0);
                ui.label(
                    RichText::new("此視窗不會重試關機。固定每週排程保持不變。")
                        .size(14.0)
                        .color(MUTED),
                );
                if ui
                    .add_sized(
                        [ui.available_width(), 46.0],
                        egui::Button::new(
                            RichText::new("關閉通知").size(17.0).color(Color32::WHITE),
                        )
                        .fill(BLUE)
                        .corner_radius(9),
                    )
                    .clicked()
                {
                    close = true;
                }
            });
        });
    close
}

pub fn builder(urgent: bool) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("關機倒數通知")
        .with_inner_size([560.0, 530.0])
        .with_min_inner_size([560.0, 530.0])
        .with_resizable(false)
        .with_minimize_button(false)
        .with_active(urgent)
        .with_window_level(if urgent {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        })
}

pub fn request_attention(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::AlwaysOnTop,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Critical,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

pub fn draw(
    ctx: &egui::Context,
    remaining: Duration,
    target: DateTime<Local>,
    simulation: bool,
    feedback: &str,
) -> Option<Action> {
    if ctx.input(|i| i.viewport().close_requested()) {
        return Some(Action::Cancel);
    }
    let mut action = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(24))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 12.0);
            ui.label(
                RichText::new(if simulation {
                    "關機提醒 · 模擬模式"
                } else {
                    "即將自動關機"
                })
                .size(25.0)
                .strong()
                .color(INK),
            );
            ui.label(
                RichText::new(if simulation {
                    "這次只測試倒數，不會關閉電腦。"
                } else {
                    "請先儲存工作，或延後這次關機。"
                })
                .size(16.0)
                .color(MUTED),
            );
            egui::Frame::new()
                .fill(NAVY)
                .corner_radius(14)
                .inner_margin(18)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("剩餘時間")
                                .size(14.0)
                                .color(Color32::from_rgb(194, 207, 231)),
                        );
                        ui.label(
                            RichText::new(format_remaining(remaining))
                                .monospace()
                                .size(52.0)
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new(format!("預定 {}", target.format("%m/%d %H:%M:%S")))
                                .size(16.0)
                                .color(Color32::from_rgb(213, 224, 244)),
                        );
                    });
                });
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 10.0) / 2.0;
                for minutes in [10, 30] {
                    if ui
                        .add_sized(
                            [width, 48.0],
                            egui::Button::new(
                                RichText::new(format!("延後 {minutes} 分鐘"))
                                    .size(17.0)
                                    .color(BLUE),
                            )
                            .fill(Color32::WHITE)
                            .stroke(Stroke::new(1.0_f32, LINE))
                            .corner_radius(9),
                        )
                        .clicked()
                    {
                        action = Some(Action::Extend(Duration::from_secs(minutes * 60)));
                    }
                }
            });
            if ui
                .add_sized(
                    [ui.available_width(), 46.0],
                    egui::Button::new(
                        RichText::new("取消本次關機")
                            .size(17.0)
                            .color(Color32::WHITE),
                    )
                    .fill(BLUE)
                    .corner_radius(9),
                )
                .clicked()
            {
                action = Some(Action::Cancel);
            }
            ui.label(
                RichText::new(
                    "延後會累加到目前預定時間，固定每週排程不變。\n關閉此通知視窗，也會取消本次。",
                )
                .size(14.0)
                .color(MUTED),
            );
            if !feedback.is_empty() {
                ui.label(RichText::new(feedback).size(14.0).color(INK));
            }
        });
    ctx.request_repaint_after(Duration::from_millis(200));
    action
}

pub fn run_weekly(session: ScheduledReminder) -> eframe::Result {
    let duration = session
        .target
        .signed_duration_since(Local::now())
        .to_std()
        .unwrap_or(Duration::from_millis(1));
    let mode = if session.simulation {
        Mode::Simulation
    } else {
        Mode::Shutdown
    };
    let mut timer = Timer::default();
    timer
        .start(Instant::now(), duration.max(Duration::from_millis(1)), mode)
        .map_err(|error| eframe::Error::AppCreation(error.into()))?;
    eframe::run_native(
        "關機倒數通知",
        eframe::NativeOptions {
            viewport: builder(true),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(move |cc| {
            crate::app::configure_theme(&cc.egui_ctx);
            Ok(Box::new(WeeklyNotification {
                session: Some(session),
                timer,
                state: WindowState::default(),
                feedback: String::new(),
                terminal: None,
            }))
        }),
    )
}

struct WeeklyNotification {
    session: Option<ScheduledReminder>,
    timer: Timer,
    state: WindowState,
    feedback: String,
    terminal: Option<TerminalNotice>,
}

impl eframe::App for WeeklyNotification {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(notice) = &self.terminal {
            if draw_terminal(ctx, notice) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let Some(session) = self.session.as_mut() else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        let now = Instant::now();
        let remaining = self.timer.remaining(now).unwrap_or_default();
        if self.state.observe(remaining) {
            request_attention(ctx);
        }
        let action = draw(
            ctx,
            remaining,
            session.target,
            session.simulation,
            &self.feedback,
        );
        self.state.mark_shown(Instant::now());
        match action {
            Some(Action::Cancel) => {
                self.timer.cancel();
                let result = session.cancel();
                self.session = None;
                match result {
                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(error) => {
                        self.terminal = Some(TerminalNotice::new("本次已取消，紀錄未保存", error));
                        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            egui::WindowLevel::Normal,
                        ));
                        ctx.request_repaint();
                    }
                }
                return;
            }
            Some(Action::Extend(extra)) => match self.timer.extend(Instant::now(), extra) {
                Ok(remaining) => {
                    session.target =
                        Local::now() + chrono::Duration::from_std(remaining).unwrap_or_default();
                    self.feedback = format!(
                        "已延後 {} 分鐘，本次預定 {}。",
                        extra.as_secs() / 60,
                        session.target.format("%H:%M")
                    );
                    self.state.observe(remaining);
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        egui::WindowLevel::Normal,
                    ));
                }
                Err(error) => self.feedback = error,
            },
            None => {}
        }
        if self.timer.remaining(Instant::now()) == Some(Duration::ZERO)
            && !self.state.can_execute(Instant::now())
        {
            self.timer.cancel();
            let result = session.cancel();
            self.session = None;
            let message = match result {
                Ok(()) => {
                    "通知未能及時顯示，沒有足夠的操作時間，因此已取消本次。請重新設定。".into()
                }
                Err(error) => format!("通知未能及時顯示，已取消本次；紀錄未保存：{error}"),
            };
            self.terminal = Some(TerminalNotice::new("本次已取消", message));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            return;
        }
        if let Some(result) = self.timer.poll(Instant::now(), shutdown::execute) {
            let execution = result.map(|_| ());
            let recorded = session.finish(execution.clone());
            let title = if execution.is_err() {
                "本次關機失敗"
            } else if session.simulation {
                "模擬完成，紀錄未保存"
            } else {
                "關機要求已送出，紀錄未保存"
            };
            let error = match (execution, recorded) {
                (Err(error), Err(record_error)) => {
                    Some(format!("{error}\n紀錄也未能保存：{record_error}"))
                }
                (Err(error), Ok(())) | (Ok(()), Err(error)) => Some(error),
                (Ok(()), Ok(())) => None,
            };
            self.session = None;
            if let Some(error) = error {
                self.terminal = Some(TerminalNotice::new(title, error));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_reappears_after_each_extension_without_early_attention() {
        let mut state = WindowState::default();
        assert!(!state.observe(Duration::from_secs(301)));
        assert!(!state.opened);
        assert!(state.observe(Duration::from_secs(300)));
        assert!(!state.observe(Duration::from_secs(299)));
        assert!(!state.observe(Duration::from_secs(899)));
        assert!(state.opened);
        assert!(!state.urgent);
        assert!(state.observe(Duration::from_secs(300)));
    }

    #[test]
    fn deadline_cannot_execute_without_thirty_seconds_of_notice() {
        let now = Instant::now();
        let mut state = WindowState::default();
        assert!(!state.can_execute(now));
        state.mark_shown(now);
        assert!(!state.can_execute(now + Duration::from_secs(29)));
        state.mark_shown(now + Duration::from_secs(20));
        assert!(state.can_execute(now + Duration::from_secs(30)));
    }

    struct PaintedText {
        value: String,
        bounds: egui::Rect,
        clip: egui::Rect,
        elided: bool,
    }

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560.0, 530.0))
    }

    fn input(scale: f32, frame: usize, events: Vec<egui::Event>) -> egui::RawInput {
        let mut input = egui::RawInput {
            screen_rect: Some(viewport()),
            time: Some(frame as f64 / 60.0),
            events,
            ..Default::default()
        };
        let info = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        info.native_pixels_per_point = Some(scale);
        info.inner_rect = Some(viewport());
        input
    }

    fn collect_text(shape: &egui::epaint::Shape, clip: egui::Rect, texts: &mut Vec<PaintedText>) {
        match shape {
            egui::epaint::Shape::Text(shape) => texts.push(PaintedText {
                value: shape.galley.job.text.clone(),
                bounds: shape.visual_bounding_rect(),
                clip,
                elided: shape.galley.elided,
            }),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, clip, texts);
                }
            }
            _ => {}
        }
    }

    fn painted_text(output: &egui::FullOutput) -> Vec<PaintedText> {
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, clipped.clip_rect, &mut texts);
        }
        texts
    }

    fn assert_horizontal_bounds(texts: &[PaintedText], scale: f32) {
        let tolerance = 2.0 / scale;
        for text in texts {
            assert!(
                !text.elided,
                "text was truncated at scale {scale}: {}",
                text.value
            );
            if !text.bounds.is_positive() {
                continue;
            }
            assert!(
                text.bounds.left() >= viewport().left() - tolerance
                    && text.bounds.right() <= viewport().right() + tolerance
                    && text.bounds.left() >= text.clip.left() - tolerance
                    && text.bounds.right() <= text.clip.right() + tolerance,
                "text overflows horizontally at scale {scale}: {:?}, bounds {:?}, clip {:?}",
                text.value,
                text.bounds,
                text.clip
            );
        }
    }

    fn fully_visible(text: &PaintedText, scale: f32) -> bool {
        let tolerance = 2.0 / scale;
        viewport().expand(tolerance).contains_rect(text.bounds)
            && text.clip.expand(tolerance).contains_rect(text.bounds)
    }

    // These are headless egui layout/paint checks at native pixel ratios, not OS DPI tests.
    #[test]
    fn notification_text_and_actions_fit_the_actual_painted_viewport_at_three_scales() {
        for scale in [1.0_f32, 1.25, 1.5] {
            let ctx = egui::Context::default();
            crate::app::configure_theme(&ctx);
            let target = Local::now() + chrono::Duration::minutes(15);
            for (case, simulation, feedback) in [
                (0, true, ""),
                (1, false, ""),
                (2, true, "已延後 30 分鐘，本次預定 00:20。"),
                (3, false, "延長後的剩餘時間不得超過 7 天。"),
            ] {
                let render = |ctx: &egui::Context| {
                    let _ = draw(ctx, Duration::from_secs(899), target, simulation, feedback);
                };
                let _ = ctx.run(input(scale, case * 2, Vec::new()), render);
                let output = ctx.run(input(scale, case * 2 + 1, Vec::new()), render);
                assert!((output.pixels_per_point - scale).abs() < f32::EPSILON);
                let texts = painted_text(&output);
                assert_horizontal_bounds(&texts, scale);
                for text in &texts {
                    assert!(
                        fully_visible(text, scale),
                        "text is clipped vertically at scale {scale}: {:?}, bounds {:?}, clip {:?}",
                        text.value,
                        text.bounds,
                        text.clip
                    );
                }
                for action in ["延後 10 分鐘", "延後 30 分鐘", "取消本次關機"] {
                    assert!(
                        texts.iter().any(|text| text.value == action),
                        "missing {action}"
                    );
                }
                if !feedback.is_empty() {
                    assert!(texts.iter().any(|text| text.value == feedback));
                }
            }
            #[cfg(windows)]
            if std::env::var_os("SystemRoot").is_some_and(|root| {
                std::path::PathBuf::from(root)
                    .join("Fonts/msjh.ttc")
                    .exists()
            }) {
                assert!(ctx.fonts_mut(|fonts| {
                    fonts.has_glyphs(&egui::FontId::proportional(16.0), "關機通知延後取消")
                }));
            }
        }
    }

    #[test]
    fn long_terminal_errors_can_scroll_to_a_visible_and_working_close_button() {
        for scale in [1.0_f32, 1.25, 1.5] {
            let ctx = egui::Context::default();
            crate::app::configure_theme(&ctx);
            let notice = TerminalNotice::new(
                "本次已取消，紀錄未保存",
                "Windows 無法保存執行紀錄。請確認目前使用者具有資料夾的寫入權限；本次已取消，不會重試關機。\n".repeat(45),
            );
            let _ = ctx.run(input(scale, 0, Vec::new()), |ctx| {
                draw_terminal(ctx, &notice);
            });
            let output = ctx.run(input(scale, 1, Vec::new()), |ctx| {
                draw_terminal(ctx, &notice);
            });
            let texts = painted_text(&output);
            assert_horizontal_bounds(&texts, scale);
            assert!(texts.iter().any(|text| text.value == notice.message));

            let mut output = ctx.run(
                input(
                    scale,
                    2,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(200.0, 250.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -100_000.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                ),
                |ctx| {
                    draw_terminal(ctx, &notice);
                },
            );
            for frame in 3..15 {
                output = ctx.run(input(scale, frame, Vec::new()), |ctx| {
                    draw_terminal(ctx, &notice);
                });
            }
            let texts = painted_text(&output);
            assert_horizontal_bounds(&texts, scale);
            let close = texts
                .iter()
                .find(|text| text.value == "關閉通知")
                .expect("close action must be painted after scrolling to the end");
            assert!(
                fully_visible(close, scale),
                "close button is clipped at scale {scale}"
            );
            let position = close.bounds.center();
            let mut closed = false;
            for (frame, pressed) in [(15, true), (16, false)] {
                let _ = ctx.run(
                    input(
                        scale,
                        frame,
                        vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::PointerButton {
                                pos: position,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    ),
                    |ctx| {
                        closed |= draw_terminal(ctx, &notice);
                    },
                );
            }
            assert!(
                closed,
                "visible close button did not accept input at scale {scale}"
            );
        }
    }

    fn luminance(color: Color32) -> f64 {
        let linear = |component: u8| {
            let value = f64::from(component) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        linear(color.r()) * 0.2126 + linear(color.g()) * 0.7152 + linear(color.b()) * 0.0722
    }

    fn contrast(foreground: Color32, background: Color32) -> f64 {
        let a = luminance(foreground);
        let b = luminance(background);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn primary_text_palette_meets_wcag_contrast_thresholds() {
        let ctx = egui::Context::default();
        crate::app::configure_theme(&ctx);
        let style = ctx.style();
        let body = style.visuals.override_text_color.unwrap();
        let action = style.visuals.selection.bg_fill;
        for (name, foreground, background) in [
            ("main body on panel", body, Color32::WHITE),
            ("main body on background", body, BACKGROUND),
            ("caption on panel", MUTED, Color32::WHITE),
            ("caption on background", MUTED, BACKGROUND),
            ("primary action", Color32::WHITE, action),
            ("secondary action", BLUE, Color32::WHITE),
            ("countdown caption", Color32::from_rgb(194, 207, 231), NAVY),
            ("countdown target", Color32::from_rgb(213, 224, 244), NAVY),
        ] {
            assert!(
                contrast(foreground, background) >= 4.5,
                "{name} contrast is below 4.5"
            );
        }
        assert!(
            contrast(Color32::WHITE, NAVY) >= 3.0,
            "large countdown contrast is below 3"
        );
    }
}
