use crate::ipc;
use core_graphics::display::CGDisplay;
use eframe::egui;
use std::f32::consts::TAU;
use std::sync::mpsc;
use std::time::Instant;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Emergency unlock: press Escape 3 times within 2 seconds
const ESCAPE_COUNT_REQUIRED: u32 = 3;
const ESCAPE_WINDOW_SECS: f64 = 2.0;

// ── Color palette ───────────────────────────────────────────────────────
const BG: egui::Color32 = egui::Color32::from_rgb(18, 18, 24);
const BG_CARD: egui::Color32 = egui::Color32::from_rgb(28, 28, 40);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(99, 102, 241);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(240, 240, 250);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(140, 140, 160);
const GREEN: egui::Color32 = egui::Color32::from_rgb(52, 211, 153);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(251, 191, 36);
const RED: egui::Color32 = egui::Color32::from_rgb(248, 113, 113);
const RING_BG: egui::Color32 = egui::Color32::from_rgb(40, 40, 56);
const OVERLAY_BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 16);

// ── Timer state ─────────────────────────────────────────────────────────
#[derive(PartialEq, Clone, Copy)]
enum Phase {
    Setup,
    Running,
    Paused,
    BreakScreen,
    Done,
}

struct TimerState {
    phase: Phase,
    total_secs: u64,
    start_time: Option<Instant>,
    elapsed_before_pause: u64,
    lockout_secs: u64,
    break_start: Option<Instant>,
    warned: bool,
}

impl TimerState {
    fn new() -> Self {
        Self {
            phase: Phase::Setup,
            total_secs: 0,
            start_time: None,
            elapsed_before_pause: 0,
            lockout_secs: 30,
            break_start: None,
            warned: false,
        }
    }

    fn elapsed(&self) -> u64 {
        match self.phase {
            Phase::Running => {
                self.elapsed_before_pause
                    + self.start_time.map_or(0, |t| t.elapsed().as_secs())
            }
            Phase::Paused => self.elapsed_before_pause,
            _ => 0,
        }
    }

    fn remaining(&self) -> u64 {
        self.total_secs.saturating_sub(self.elapsed())
    }

    fn progress(&self) -> f32 {
        if self.total_secs == 0 {
            return 0.0;
        }
        1.0 - (self.remaining() as f32 / self.total_secs as f32)
    }

    fn start(&mut self, secs: u64) {
        self.total_secs = secs;
        self.elapsed_before_pause = 0;
        self.start_time = Some(Instant::now());
        self.warned = false;
        self.phase = Phase::Running;
        notify("Screen Timer", &format!("Timer started: {}", format_time(secs)));
    }

    fn pause(&mut self) {
        self.elapsed_before_pause = self.elapsed();
        self.phase = Phase::Paused;
    }

    fn resume(&mut self) {
        self.start_time = Some(Instant::now());
        self.phase = Phase::Running;
    }

    fn stop(&mut self) {
        self.phase = Phase::Setup;
        self.start_time = None;
        self.elapsed_before_pause = 0;
        self.warned = false;
    }

    fn enter_break(&mut self) {
        self.break_start = Some(Instant::now());
        self.phase = Phase::BreakScreen;
        notify("Screen Timer", "Time's up! Take a break.");
    }

    fn break_remaining(&self) -> u64 {
        self.break_start
            .map(|t| self.lockout_secs.saturating_sub(t.elapsed().as_secs()))
            .unwrap_or(0)
    }
}

// ── Tray menu item IDs ─────────────────────────────────────────────────
struct TrayMenuIds {
    start_25m: MenuItem,
    start_45m: MenuItem,
    start_1h: MenuItem,
    start_1h30m: MenuItem,
    start_2h: MenuItem,
    pause_resume: MenuItem,
    stop: MenuItem,
    show_window: MenuItem,
    quit: MenuItem,
}

// ── Main app ────────────────────────────────────────────────────────────
pub struct App {
    timer: TimerState,
    hours: u32,
    minutes: u32,
    seconds: u32,
    message: String,
    anim_time: f64,

    // Tray
    tray: Option<TrayIcon>,
    tray_ids: Option<TrayMenuIds>,

    // IPC
    ipc_rx: mpsc::Receiver<ipc::IpcCommand>,
    ipc_tx: Option<mpsc::Sender<ipc::IpcCommand>>,
    ipc_started: bool,

    // Break screen
    break_texture: Option<egui::TextureHandle>,
    break_image_loaded: bool,
    was_fullscreen: bool,

    // Emergency unlock (triple-Escape)
    escape_count: u32,
    last_escape_time: Option<Instant>,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            timer: TimerState::new(),
            hours: 0,
            minutes: 25,
            seconds: 0,
            message: String::new(),
            anim_time: 0.0,
            tray: None,
            tray_ids: None,
            ipc_rx: rx,
            ipc_tx: Some(tx),
            ipc_started: false,
            break_texture: None,
            break_image_loaded: false,
            was_fullscreen: false,
            escape_count: 0,
            last_escape_time: None,
        }
    }

    fn total_input_secs(&self) -> u64 {
        self.hours as u64 * 3600 + self.minutes as u64 * 60 + self.seconds as u64
    }

    fn start_timer_from_input(&mut self) {
        let total = self.total_input_secs();
        if total < 10 {
            self.message = "Minimum 10 seconds".to_string();
            return;
        }
        self.message.clear();
        self.timer.start(total);
    }

    fn handle_ipc_commands(&mut self) {
        while let Ok(cmd) = self.ipc_rx.try_recv() {
            match cmd {
                ipc::IpcCommand::Start { seconds } => {
                    self.timer.start(seconds);
                }
                ipc::IpcCommand::Stop => {
                    self.timer.stop();
                }
                ipc::IpcCommand::Pause => {
                    if self.timer.phase == Phase::Running {
                        self.timer.pause();
                    }
                }
                ipc::IpcCommand::Resume => {
                    if self.timer.phase == Phase::Paused {
                        self.timer.resume();
                    }
                }
                ipc::IpcCommand::Status => {
                    // Status is handled inline by IPC server for now
                }
                ipc::IpcCommand::SetImage { path: _ } => {
                    // Future: set break image
                }
                ipc::IpcCommand::Unlock => {
                    if self.timer.phase == Phase::BreakScreen {
                        self.timer.phase = Phase::Done;
                    }
                }
                ipc::IpcCommand::Quit => {
                    std::process::exit(0);
                }
            }
        }
    }

    fn update_tray(&mut self) {
        let Some(tray) = &self.tray else { return };
        let Some(ids) = &self.tray_ids else { return };

        match self.timer.phase {
            Phase::Running => {
                let remaining = format_time(self.timer.remaining());
                let _ = tray.set_title(Some(&format!("  {remaining}")));
                ids.pause_resume.set_text("Pause");
                ids.pause_resume.set_enabled(true);
                ids.stop.set_enabled(true);
                ids.start_25m.set_enabled(false);
                ids.start_45m.set_enabled(false);
                ids.start_1h.set_enabled(false);
                ids.start_1h30m.set_enabled(false);
                ids.start_2h.set_enabled(false);
            }
            Phase::Paused => {
                let remaining = format_time(self.timer.remaining());
                let _ = tray.set_title(Some(&format!("  {remaining} ⏸")));
                ids.pause_resume.set_text("Resume");
                ids.pause_resume.set_enabled(true);
                ids.stop.set_enabled(true);
            }
            Phase::BreakScreen => {
                let _ = tray.set_title(Some("  BREAK"));
            }
            _ => {
                let _ = tray.set_title(Some(""));
                ids.pause_resume.set_enabled(false);
                ids.stop.set_enabled(false);
                ids.start_25m.set_enabled(true);
                ids.start_45m.set_enabled(true);
                ids.start_1h.set_enabled(true);
                ids.start_1h30m.set_enabled(true);
                ids.start_2h.set_enabled(true);
            }
        }
    }

    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        let Some(ids) = &self.tray_ids else { return };

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id().clone();
            if id == *ids.start_25m.id() {
                self.timer.start(25 * 60);
            } else if id == *ids.start_45m.id() {
                self.timer.start(45 * 60);
            } else if id == *ids.start_1h.id() {
                self.timer.start(60 * 60);
            } else if id == *ids.start_1h30m.id() {
                self.timer.start(90 * 60);
            } else if id == *ids.start_2h.id() {
                self.timer.start(120 * 60);
            } else if id == *ids.pause_resume.id() {
                match self.timer.phase {
                    Phase::Running => self.timer.pause(),
                    Phase::Paused => self.timer.resume(),
                    _ => {}
                }
            } else if id == *ids.stop.id() {
                self.timer.stop();
            } else if id == *ids.show_window.id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if id == *ids.quit.id() {
                ipc::cleanup();
                std::process::exit(0);
            }
        }
    }

    fn load_break_image(&mut self, ctx: &egui::Context) {
        if self.break_image_loaded {
            return;
        }
        self.break_image_loaded = true;

        let images_dir = ipc::images_dir();
        let entries: Vec<_> = std::fs::read_dir(&images_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let p = e.path();
                matches!(
                    p.extension().and_then(|s| s.to_str()),
                    Some("png" | "jpg" | "jpeg" | "webp")
                )
            })
            .collect();

        if entries.is_empty() {
            return;
        }

        // Pick a pseudo-random image
        let idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as usize
            % entries.len();

        let path = entries[idx].path();
        if let Ok(img) = image::open(&path) {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels = rgba.as_flat_samples();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
            self.break_texture = Some(ctx.load_texture(
                "break-image",
                color_image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }
}

pub fn run() -> eframe::Result<()> {
    // Ensure config dirs exist
    let _ = ipc::images_dir();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 560.0])
            .with_min_inner_size([380.0, 500.0])
            .with_always_on_top(),
        ..Default::default()
    };

    eframe::run_native("Screen Timer", options, Box::new(|cc| {
        setup_visuals(&cc.egui_ctx);

        let mut app = App::new();

        // Create tray icon
        match create_tray() {
            Ok((tray, ids)) => {
                app.tray = Some(tray);
                app.tray_ids = Some(ids);
            }
            Err(e) => eprintln!("Warning: Could not create tray icon: {e}"),
        }

        // Start IPC server
        if let Some(tx) = app.ipc_tx.take() {
            ipc::start_server(tx, cc.egui_ctx.clone());
            app.ipc_started = true;
        }

        Ok(Box::new(app))
    }))
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.anim_time += ctx.input(|i| i.unstable_dt as f64);

        // Keep ticking
        if matches!(self.timer.phase, Phase::Running | Phase::BreakScreen) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Handle events
        self.handle_ipc_commands();
        self.handle_tray_events(ctx);

        // Timer expired → enter break screen
        if self.timer.phase == Phase::Running && self.timer.remaining() == 0 {
            self.break_image_loaded = false; // reload image
            self.escape_count = 0;
            self.last_escape_time = None;
            self.timer.enter_break();
            // Go fullscreen for break
            self.was_fullscreen = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        // Warning at 60s
        if self.timer.phase == Phase::Running && !self.timer.warned && self.timer.remaining() <= 60
        {
            self.timer.warned = true;
            notify("Screen Timer", "1 minute remaining! Save your work.");
        }

        // Emergency unlock: triple-Escape during break
        if self.timer.phase == Phase::BreakScreen {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                let now = Instant::now();
                if let Some(last) = self.last_escape_time {
                    if now.duration_since(last).as_secs_f64() > ESCAPE_WINDOW_SECS {
                        self.escape_count = 0; // reset if too slow
                    }
                }
                self.escape_count += 1;
                self.last_escape_time = Some(now);

                if self.escape_count >= ESCAPE_COUNT_REQUIRED {
                    // Emergency unlock!
                    self.timer.phase = Phase::Done;
                    if self.was_fullscreen {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                        self.was_fullscreen = false;
                    }
                    self.escape_count = 0;
                    notify("Screen Timer", "Emergency unlock activated.");
                }
            }
        }

        // Break screen timeout → done
        if self.timer.phase == Phase::BreakScreen && self.timer.break_remaining() == 0 {
            self.timer.phase = Phase::Done;
            if self.was_fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                self.was_fullscreen = false;
            }
        }

        // Intercept close: hide instead of quit while timer is running
        if ctx.input(|i| i.viewport().close_requested()) {
            if matches!(
                self.timer.phase,
                Phase::Running | Phase::Paused | Phase::BreakScreen
            ) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                ipc::cleanup();
            }
        }

        self.update_tray();

        // ── Draw ────────────────────────────────────────────────────────
        if self.timer.phase == Phase::BreakScreen {
            self.draw_break_fullscreen(ctx);
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BG).inner_margin(24.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Screen Timer")
                                .size(24.0)
                                .color(TEXT_PRIMARY)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Focus. Then rest.")
                                .size(13.0)
                                .color(TEXT_DIM),
                        );
                        ui.add_space(16.0);

                        match self.timer.phase {
                            Phase::Setup => self.draw_setup(ui),
                            Phase::Running | Phase::Paused => self.draw_timer(ui),
                            Phase::Done => self.draw_done(ui),
                            Phase::BreakScreen => {} // handled above
                        }

                        if !self.message.is_empty() {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(&self.message).size(13.0).color(YELLOW),
                            );
                        }
                    });
                });
        }
    }
}

impl App {
    fn draw_setup(&mut self, ui: &mut egui::Ui) {
        // Duration card
        egui::Frame::new()
            .fill(BG_CARD)
            .corner_radius(16.0)
            .inner_margin(24.0)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("SET DURATION")
                            .size(11.0)
                            .color(TEXT_DIM)
                            .strong(),
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        let avail = ui.available_width();
                        ui.add_space((avail - 240.0).max(0.0) / 2.0);
                        styled_spinner(ui, &mut self.hours, "HR", 23);
                        ui.label(egui::RichText::new(":").size(36.0).color(TEXT_DIM));
                        styled_spinner(ui, &mut self.minutes, "MIN", 59);
                        ui.label(egui::RichText::new(":").size(36.0).color(TEXT_DIM));
                        styled_spinner(ui, &mut self.seconds, "SEC", 59);
                    });
                });
            });

        ui.add_space(16.0);

        // Presets
        ui.label(egui::RichText::new("QUICK START").size(11.0).color(TEXT_DIM).strong());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            let btn_w = (avail - 40.0) / 5.0;
            for (label, h, m) in [("25m", 0u32, 25u32), ("45m", 0, 45), ("1h", 1, 0), ("1h30", 1, 30), ("2h", 2, 0)]
            {
                let sel = self.hours == h && self.minutes == m && self.seconds == 0;
                let btn = egui::Button::new(
                    egui::RichText::new(label)
                        .size(13.0)
                        .color(if sel { BG } else { TEXT_PRIMARY }),
                )
                .fill(if sel { ACCENT } else { BG_CARD })
                .corner_radius(10.0)
                .min_size(egui::vec2(btn_w, 36.0));
                if ui.add(btn).clicked() {
                    self.hours = h;
                    self.minutes = m;
                    self.seconds = 0;
                }
            }
        });

        ui.add_space(16.0);

        // Break duration
        egui::Frame::new()
            .fill(BG_CARD)
            .corner_radius(12.0)
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Break screen duration").size(13.0).color(TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("sec").size(12.0).color(TEXT_DIM));
                        ui.add(egui::DragValue::new(&mut self.timer.lockout_secs).range(5..=300));
                    });
                });
            });

        ui.add_space(8.0);

        // Images hint
        let images_dir = ipc::images_dir();
        let count = std::fs::read_dir(&images_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                matches!(
                    e.path().extension().and_then(|s| s.to_str()),
                    Some("png" | "jpg" | "jpeg" | "webp")
                )
            })
            .count();
        let img_text = if count > 0 {
            format!("{count} break image(s) found")
        } else {
            format!("Add images to {}", images_dir.display())
        };
        ui.label(egui::RichText::new(img_text).size(11.0).color(TEXT_DIM));

        ui.add_space(20.0);

        // Start button
        let btn = egui::Button::new(
            egui::RichText::new("Start Focus Session")
                .size(18.0)
                .color(egui::Color32::WHITE)
                .strong(),
        )
        .fill(ACCENT)
        .corner_radius(14.0)
        .min_size(egui::vec2(ui.available_width(), 52.0));
        if ui.add(btn).clicked() {
            self.start_timer_from_input();
        }
    }

    fn draw_timer(&mut self, ui: &mut egui::Ui) {
        let remaining = self.timer.remaining();
        let progress = self.timer.progress();

        let ring_color = if remaining <= 10 {
            RED
        } else if remaining <= 60 {
            YELLOW
        } else {
            ACCENT
        };

        // Circular timer
        let ring_size = 220.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ring_size, ring_size), egui::Sense::hover());
        let painter = ui.painter();
        let center = rect.center();
        let radius = ring_size / 2.0 - 8.0;

        painter.circle_stroke(center, radius, egui::Stroke::new(8.0, RING_BG));
        draw_arc(painter, center, radius, progress, 8.0, ring_color);

        // Pulsing glow when paused
        if self.timer.phase == Phase::Paused {
            let a = ((self.anim_time * 2.0).sin() * 0.3 + 0.3) as f32;
            painter.circle_stroke(
                center,
                radius - 8.0,
                egui::Stroke::new(2.0, ring_color.gamma_multiply(a)),
            );
        }

        let time_str = format_time(remaining);
        painter.text(
            center + egui::vec2(0.0, -8.0),
            egui::Align2::CENTER_CENTER,
            &time_str,
            egui::FontId::new(48.0, egui::FontFamily::Proportional),
            TEXT_PRIMARY,
        );
        painter.text(
            center + egui::vec2(0.0, 24.0),
            egui::Align2::CENTER_CENTER,
            &format!("of {}", format_time(self.timer.total_secs)),
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
            TEXT_DIM,
        );

        ui.add_space(16.0);

        if self.timer.phase == Phase::Paused {
            ui.label(egui::RichText::new("PAUSED").size(14.0).color(YELLOW).strong());
            ui.add_space(8.0);
        }

        // Buttons
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            let btn_w = (avail - 12.0) / 2.0;

            match self.timer.phase {
                Phase::Running => {
                    let btn = egui::Button::new(egui::RichText::new("Pause").size(15.0).color(TEXT_PRIMARY))
                        .fill(BG_CARD).corner_radius(12.0).min_size(egui::vec2(btn_w, 44.0));
                    if ui.add(btn).clicked() { self.timer.pause(); }
                }
                Phase::Paused => {
                    let btn = egui::Button::new(
                        egui::RichText::new("Resume").size(15.0).color(egui::Color32::WHITE).strong(),
                    )
                    .fill(ACCENT).corner_radius(12.0).min_size(egui::vec2(btn_w, 44.0));
                    if ui.add(btn).clicked() { self.timer.resume(); }
                }
                _ => {}
            }

            let btn = egui::Button::new(egui::RichText::new("Cancel").size(15.0).color(RED))
                .fill(BG_CARD).corner_radius(12.0).min_size(egui::vec2(btn_w, 44.0));
            if ui.add(btn).clicked() { self.timer.stop(); }
        });
    }

    fn draw_break_fullscreen(&mut self, ctx: &egui::Context) {
        self.load_break_image(ctx);

        // Create overlay viewports for ALL screens (covers multi-monitor)
        let screens = get_screen_rects();
        for (i, (x, y, w, h)) in screens.iter().enumerate() {
            let id = egui::ViewportId::from_hash_of(format!("break-overlay-{i}"));
            let builder = egui::ViewportBuilder::default()
                .with_position(egui::pos2(*x as f32, *y as f32))
                .with_inner_size(egui::vec2(*w as f32, *h as f32))
                .with_decorations(false)
                .with_always_on_top()
                .with_taskbar(false);
            ctx.show_viewport_deferred(id, builder, |ctx, _class| {
                setup_visuals(ctx);
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(OVERLAY_BG))
                    .show(ctx, |ui| {
                        let avail = ui.available_size();
                        ui.vertical_centered(|ui| {
                            ui.add_space(avail.y * 0.35);
                            ui.label(
                                egui::RichText::new("Time for a break")
                                    .size(48.0)
                                    .color(egui::Color32::from_white_alpha(100))
                                    .strong(),
                            );
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new("Step away from the screen")
                                    .size(20.0)
                                    .color(egui::Color32::from_white_alpha(60)),
                            );
                        });
                    });
            });
        }

        // Main window: rich break screen with image, progress, escape hint
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(OVERLAY_BG))
            .show(ctx, |ui| {
                let avail = ui.available_size();

                // If we have a break image, draw it as background
                if let Some(tex) = &self.break_texture {
                    let tex_size = tex.size_vec2();
                    let scale = (avail.x / tex_size.x).max(avail.y / tex_size.y);
                    let img_size = tex_size * scale;
                    let offset = egui::vec2(
                        (avail.x - img_size.x) / 2.0,
                        (avail.y - img_size.y) / 2.0,
                    );
                    let img_rect = egui::Rect::from_min_size(
                        ui.min_rect().min + offset,
                        img_size,
                    );
                    ui.painter().image(
                        tex.id(),
                        img_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    // Dark overlay for readability
                    ui.painter().rect_filled(
                        ui.min_rect(),
                        0.0,
                        egui::Color32::from_black_alpha(140),
                    );
                } else {
                    // Animated ambient circles
                    let t = self.anim_time as f32;
                    for i in 0..5 {
                        let fi = i as f32;
                        let x = avail.x * 0.5 + (t * 0.3 + fi * 1.2).sin() * avail.x * 0.2;
                        let y = avail.y * 0.5 + (t * 0.2 + fi * 0.8).cos() * avail.y * 0.2;
                        let r = 100.0 + fi * 40.0;
                        let alpha = (15.0 - fi * 2.0).max(5.0) as u8;
                        ui.painter().circle_filled(
                            ui.min_rect().min + egui::vec2(x, y),
                            r,
                            ACCENT.gamma_multiply(alpha as f32 / 255.0),
                        );
                    }
                }

                // Center content
                ui.vertical_centered(|ui| {
                    ui.add_space(avail.y * 0.3);

                    ui.label(
                        egui::RichText::new("Time for a break")
                            .size(56.0)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    );
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("Step away from the screen. Stretch. Breathe.")
                            .size(20.0)
                            .color(egui::Color32::from_white_alpha(180)),
                    );
                    ui.add_space(32.0);

                    let remaining = self.timer.break_remaining();
                    ui.label(
                        egui::RichText::new(format!("Resuming in {}s", remaining))
                            .size(18.0)
                            .color(egui::Color32::from_white_alpha(120)),
                    );

                    // Progress dots
                    ui.add_space(16.0);
                    let total = self.timer.lockout_secs as f32;
                    let elapsed = total - remaining as f32;
                    let frac = if total > 0.0 { elapsed / total } else { 0.0 };
                    let dot_count = 20;
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(dot_count as f32 * 16.0, 8.0),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter();
                    for i in 0..dot_count {
                        let x = dot_rect.min.x + i as f32 * 16.0 + 4.0;
                        let y = dot_rect.center().y;
                        let filled = (i as f32 / dot_count as f32) < frac;
                        let color = if filled {
                            GREEN.gamma_multiply(0.9)
                        } else {
                            egui::Color32::from_white_alpha(30)
                        };
                        painter.circle_filled(egui::pos2(x, y), 4.0, color);
                    }

                    // Emergency unlock hint + escape progress
                    ui.add_space(40.0);
                    let esc_hint = if self.escape_count > 0 && self.escape_count < ESCAPE_COUNT_REQUIRED {
                        format!(
                            "Press Esc {} more time(s) to unlock",
                            ESCAPE_COUNT_REQUIRED - self.escape_count
                        )
                    } else {
                        "Press Esc x3 to emergency unlock".to_string()
                    };
                    ui.label(
                        egui::RichText::new(esc_hint)
                            .size(13.0)
                            .color(egui::Color32::from_white_alpha(50)),
                    );
                });
            });
    }

    fn draw_done(&mut self, ui: &mut egui::Ui) {
        ui.add_space(30.0);

        let size = 120.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let painter = ui.painter();
        let center = rect.center();
        painter.circle_filled(center, size / 2.0, GREEN.gamma_multiply(0.15));
        painter.circle_stroke(center, size / 2.0, egui::Stroke::new(3.0, GREEN));

        let s = 20.0;
        painter.line(
            vec![
                center + egui::vec2(-s, 0.0),
                center + egui::vec2(-s * 0.3, s * 0.6),
                center + egui::vec2(s, -s * 0.5),
            ],
            egui::Stroke::new(4.0, GREEN),
        );

        ui.add_space(24.0);
        ui.label(egui::RichText::new("Break complete!").size(24.0).color(GREEN).strong());
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Welcome back. Hope you had a good rest.")
                .size(14.0)
                .color(TEXT_DIM),
        );
        ui.add_space(32.0);

        let btn = egui::Button::new(
            egui::RichText::new("Start New Session").size(16.0).color(egui::Color32::WHITE).strong(),
        )
        .fill(ACCENT)
        .corner_radius(14.0)
        .min_size(egui::vec2(220.0, 48.0));
        if ui.add(btn).clicked() {
            self.timer.stop();
        }
    }
}

// ── Tray icon creation ──────────────────────────────────────────────────
fn create_tray() -> Result<(TrayIcon, TrayMenuIds), Box<dyn std::error::Error>> {
    let menu = Menu::new();

    let start_25m = MenuItem::new("Start 25m", true, None);
    let start_45m = MenuItem::new("Start 45m", true, None);
    let start_1h = MenuItem::new("Start 1h", true, None);
    let start_1h30m = MenuItem::new("Start 1h30m", true, None);
    let start_2h = MenuItem::new("Start 2h", true, None);
    let pause_resume = MenuItem::new("Pause", false, None);
    let stop = MenuItem::new("Stop", false, None);
    let show_window = MenuItem::new("Show Window", true, None);
    let quit = MenuItem::new("Quit", true, None);

    menu.append(&start_25m)?;
    menu.append(&start_45m)?;
    menu.append(&start_1h)?;
    menu.append(&start_1h30m)?;
    menu.append(&start_2h)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&pause_resume)?;
    menu.append(&stop)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&show_window)?;
    menu.append(&quit)?;

    let icon = create_timer_icon();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("Screen Timer")
        .build()?;

    Ok((
        tray,
        TrayMenuIds {
            start_25m,
            start_45m,
            start_1h,
            start_1h30m,
            start_2h,
            pause_resume,
            stop,
            show_window,
            quit,
        },
    ))
}

fn create_timer_icon() -> Icon {
    // Create a simple 22x22 timer icon (circle with hands)
    let size = 22u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let center = size as f32 / 2.0;
    let radius = center - 1.5;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;

            if (dist - radius).abs() < 1.5 {
                // Circle outline
                let a = (1.0 - (dist - radius).abs() / 1.5).max(0.0);
                rgba[idx] = 200;
                rgba[idx + 1] = 200;
                rgba[idx + 2] = 220;
                rgba[idx + 3] = (a * 255.0) as u8;
            } else if dist < radius - 1.0 {
                // Inside: draw clock hands
                let angle_h = std::f32::consts::FRAC_PI_4; // ~1:30
                let hx = angle_h.sin() * radius * 0.45;
                let hy = -angle_h.cos() * radius * 0.45;
                let angle_m = std::f32::consts::PI;
                let mx = angle_m.sin() * radius * 0.65;
                let my = -angle_m.cos() * radius * 0.65;

                let dist_to_h = point_to_line_dist(dx, dy, 0.0, 0.0, hx, hy);
                let dist_to_m = point_to_line_dist(dx, dy, 0.0, 0.0, mx, my);

                if dist_to_h < 1.2 || dist_to_m < 1.0 {
                    rgba[idx] = 200;
                    rgba[idx + 1] = 200;
                    rgba[idx + 2] = 220;
                    rgba[idx + 3] = 200;
                }
            }
        }
    }

    Icon::from_rgba(rgba, size, size).unwrap_or_else(|_| {
        // Fallback: 1x1 icon
        Icon::from_rgba(vec![200, 200, 220, 255], 1, 1).unwrap()
    })
}

fn point_to_line_dist(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// ── Drawing helpers ─────────────────────────────────────────────────────
fn draw_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    fraction: f32,
    stroke_width: f32,
    color: egui::Color32,
) {
    if fraction <= 0.0 {
        return;
    }
    let start = -TAU / 4.0;
    let end = start + TAU * fraction.min(1.0);
    let segments = (fraction * 80.0).max(20.0) as usize;
    let points: Vec<egui::Pos2> = (0..=segments)
        .map(|i| {
            let t = i as f32 / segments as f32;
            let angle = start + (end - start) * t;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();

    if points.len() >= 2 {
        // Glow
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(stroke_width + 6.0, color.gamma_multiply(0.15)),
        ));
        // Main
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(stroke_width, color),
        ));
    }
}

fn styled_spinner(ui: &mut egui::Ui, value: &mut u32, label: &str, max: u32) {
    ui.vertical(|ui| {
        ui.add(
            egui::DragValue::new(value)
                .range(0..=max)
                .speed(0.3)
                .custom_formatter(|v, _| format!("{:02}", v as u32)),
        );
        ui.label(egui::RichText::new(label).size(10.0).color(TEXT_DIM));
    });
}

fn setup_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = BG_CARD;
    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 65);
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(45, 45, 65);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 55, 75);
    visuals.widgets.active.bg_fill = ACCENT;
    let r = egui::CornerRadius::same(12);
    visuals.widgets.inactive.corner_radius = r;
    visuals.widgets.hovered.corner_radius = r;
    visuals.widgets.active.corner_radius = r;
    visuals.widgets.noninteractive.corner_radius = r;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    ctx.set_style(style);
}

fn format_time(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Get all screen rects as (x, y, width, height) in logical coordinates
fn get_screen_rects() -> Vec<(f64, f64, f64, f64)> {
    CGDisplay::active_displays()
        .unwrap_or_default()
        .iter()
        .map(|&id| {
            let b = CGDisplay::new(id).bounds();
            (b.origin.x, b.origin.y, b.size.width, b.size.height)
        })
        .collect()
}

fn notify(title: &str, msg: &str) {
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(format!(
            r#"display notification "{msg}" with title "{title}" sound name "Glass""#
        ))
        .output();
}
