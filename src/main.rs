use eframe::egui;
use std::process::Command;
use std::time::Instant;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 500.0])
            .with_min_inner_size([350.0, 400.0])
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "Screen Timer",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

fn setup_fonts(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(28.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}

#[derive(PartialEq)]
enum Phase {
    Setup,
    Running,
    Paused,
    Lockout,
    Done,
}

struct App {
    hours: u32,
    minutes: u32,
    seconds: u32,
    lockout_secs: u64,

    phase: Phase,
    total_secs: u64,
    start_time: Option<Instant>,
    elapsed_before_pause: u64,
    pause_time: Option<Instant>,
    lockout_start: Option<Instant>,
    warned: bool,
    message: String,
}

impl App {
    fn new() -> Self {
        Self {
            hours: 0,
            minutes: 25,
            seconds: 0,
            lockout_secs: 10,
            phase: Phase::Setup,
            total_secs: 0,
            start_time: None,
            elapsed_before_pause: 0,
            pause_time: None,
            lockout_start: None,
            warned: false,
            message: String::new(),
        }
    }

    fn total_input_secs(&self) -> u64 {
        self.hours as u64 * 3600 + self.minutes as u64 * 60 + self.seconds as u64
    }

    fn elapsed_secs(&self) -> u64 {
        match self.phase {
            Phase::Running => {
                self.elapsed_before_pause
                    + self.start_time.map_or(0, |t| t.elapsed().as_secs())
            }
            Phase::Paused => self.elapsed_before_pause,
            _ => 0,
        }
    }

    fn remaining_secs(&self) -> u64 {
        self.total_secs.saturating_sub(self.elapsed_secs())
    }

    fn start(&mut self) {
        let total = self.total_input_secs();
        if total < 10 {
            self.message = "Minimum duration is 10 seconds".to_string();
            return;
        }
        self.total_secs = total;
        self.elapsed_before_pause = 0;
        self.start_time = Some(Instant::now());
        self.warned = false;
        self.message.clear();
        self.phase = Phase::Running;
        notify("Screen Timer", &format!("Timer started: {}", format_time(total)));
    }

    fn pause(&mut self) {
        self.elapsed_before_pause = self.elapsed_secs();
        self.pause_time = Some(Instant::now());
        self.phase = Phase::Paused;
    }

    fn resume(&mut self) {
        self.start_time = Some(Instant::now());
        self.phase = Phase::Running;
    }

    fn cancel(&mut self) {
        self.phase = Phase::Setup;
        self.start_time = None;
        self.elapsed_before_pause = 0;
        self.warned = false;
        self.message = "Timer cancelled".to_string();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request repaint every second while active
        if self.phase == Phase::Running || self.phase == Phase::Lockout {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        // Check if timer expired
        if self.phase == Phase::Running && self.remaining_secs() == 0 {
            // Time's up — lock screen and enter lockout
            notify("Screen Timer", "Time's up! Take a break.");
            lock_screen();
            self.lockout_start = Some(Instant::now());
            self.phase = Phase::Lockout;
        }

        // Warning at 60s remaining
        if self.phase == Phase::Running && !self.warned && self.remaining_secs() <= 60 {
            self.warned = true;
            notify("Screen Timer", "1 minute remaining! Save your work.");
        }

        // Lockout phase: keep re-locking
        if self.phase == Phase::Lockout {
            if let Some(start) = self.lockout_start {
                if start.elapsed().as_secs() >= self.lockout_secs {
                    self.phase = Phase::Done;
                } else if start.elapsed().as_secs() % 2 == 0 {
                    lock_screen();
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.heading("Screen Timer");
                ui.add_space(10.0);

                match self.phase {
                    Phase::Setup => self.draw_setup(ui),
                    Phase::Running | Phase::Paused => self.draw_timer(ui),
                    Phase::Lockout => self.draw_lockout(ui),
                    Phase::Done => self.draw_done(ui),
                }

                if !self.message.is_empty() {
                    ui.add_space(10.0);
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 0), &self.message);
                }
            });
        });
    }
}

impl App {
    fn draw_setup(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label("Set duration:");
        ui.add_space(10.0);

        // Time input with spinners
        ui.horizontal(|ui| {
            ui.add_space(50.0);
            time_spinner(ui, "h", &mut self.hours, 0, 23);
            ui.label(":");
            time_spinner(ui, "m", &mut self.minutes, 0, 59);
            ui.label(":");
            time_spinner(ui, "s", &mut self.seconds, 0, 59);
        });

        ui.add_space(10.0);

        // Quick presets
        ui.label("Quick presets:");
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.add_space(30.0);
            if ui.button("25m").clicked() {
                self.hours = 0;
                self.minutes = 25;
                self.seconds = 0;
            }
            if ui.button("45m").clicked() {
                self.hours = 0;
                self.minutes = 45;
                self.seconds = 0;
            }
            if ui.button("1h").clicked() {
                self.hours = 1;
                self.minutes = 0;
                self.seconds = 0;
            }
            if ui.button("1h30m").clicked() {
                self.hours = 1;
                self.minutes = 30;
                self.seconds = 0;
            }
            if ui.button("2h").clicked() {
                self.hours = 2;
                self.minutes = 0;
                self.seconds = 0;
            }
        });

        ui.add_space(15.0);

        // Lockout setting
        ui.horizontal(|ui| {
            ui.add_space(50.0);
            ui.label("Lockout (re-lock) seconds:");
            ui.add(egui::DragValue::new(&mut self.lockout_secs).range(5..=120));
        });

        ui.add_space(30.0);

        let btn = egui::Button::new(
            egui::RichText::new("Start Timer").size(20.0),
        )
        .min_size(egui::vec2(200.0, 50.0));

        if ui.add(btn).clicked() {
            self.start();
        }
    }

    fn draw_timer(&mut self, ui: &mut egui::Ui) {
        let remaining = self.remaining_secs();
        let elapsed = self.elapsed_secs();
        let progress = if self.total_secs > 0 {
            elapsed as f32 / self.total_secs as f32
        } else {
            0.0
        };

        // Big countdown display
        let time_str = format_time(remaining);
        let color = if remaining <= 10 {
            egui::Color32::from_rgb(255, 80, 80)
        } else if remaining <= 60 {
            egui::Color32::from_rgb(255, 200, 0)
        } else {
            egui::Color32::from_rgb(100, 220, 100)
        };

        ui.add_space(30.0);
        ui.label(
            egui::RichText::new(&time_str)
                .size(64.0)
                .color(color)
                .strong(),
        );
        ui.add_space(5.0);
        ui.label(format!("of {}", format_time(self.total_secs)));

        ui.add_space(20.0);

        // Progress bar
        let bar = egui::ProgressBar::new(progress)
            .show_percentage()
            .animate(self.phase == Phase::Running);
        ui.add_sized([300.0, 20.0], bar);

        ui.add_space(30.0);

        // Controls
        ui.horizontal(|ui| {
            ui.add_space(60.0);
            match self.phase {
                Phase::Running => {
                    if ui
                        .add(egui::Button::new("Pause").min_size(egui::vec2(100.0, 40.0)))
                        .clicked()
                    {
                        self.pause();
                    }
                }
                Phase::Paused => {
                    if ui
                        .add(egui::Button::new("Resume").min_size(egui::vec2(100.0, 40.0)))
                        .clicked()
                    {
                        self.resume();
                    }
                }
                _ => {}
            }

            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(100.0, 40.0)))
                .clicked()
            {
                self.cancel();
            }
        });

        if self.phase == Phase::Paused {
            ui.add_space(10.0);
            ui.colored_label(egui::Color32::from_rgb(255, 200, 0), "PAUSED");
        }
    }

    fn draw_lockout(&self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.label(
            egui::RichText::new("SCREEN LOCKED")
                .size(36.0)
                .color(egui::Color32::from_rgb(255, 80, 80))
                .strong(),
        );
        ui.add_space(20.0);

        if let Some(start) = self.lockout_start {
            let lockout_remaining = self.lockout_secs.saturating_sub(start.elapsed().as_secs());
            ui.label(format!(
                "Re-locking for {}s more...\nStep away from the screen!",
                lockout_remaining
            ));
        }
    }

    fn draw_done(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.label(
            egui::RichText::new("Break complete!")
                .size(36.0)
                .color(egui::Color32::from_rgb(100, 220, 100))
                .strong(),
        );
        ui.add_space(10.0);
        ui.label("Welcome back. Hope you had a good break.");
        ui.add_space(30.0);

        if ui
            .add(egui::Button::new("Start New Timer").min_size(egui::vec2(200.0, 50.0)))
            .clicked()
        {
            self.phase = Phase::Setup;
            self.message.clear();
        }
    }
}

fn time_spinner(ui: &mut egui::Ui, label: &str, value: &mut u32, min: u32, max: u32) {
    ui.vertical(|ui| {
        ui.add(
            egui::DragValue::new(value)
                .range(min..=max)
                .speed(0.5)
                .custom_formatter(|v, _| format!("{:02}", v as u32))
        );
        ui.label(label);
    });
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

fn lock_screen() {
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to keystroke "q" using {control down, command down}"#)
        .output();
}

fn notify(title: &str, msg: &str) {
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            r#"display notification "{msg}" with title "{title}" sound name "Glass""#
        ))
        .output();
}
