// ABOUTME: tmtools application shell: an eframe window with a module tab bar, the loaded files and runs, and the
// ABOUTME: playback bar every module shares. Register additional modules by pushing them into the registry in main.

mod fonts;
mod screenshot;

use eframe::egui;
use std::path::PathBuf;
use tmreplay::format_race_time;
use tmtools_core::{color_swatch, Module, Session, SPEEDS};

// Where the chosen asset folder is remembered between launches
const ASSETS_KEY: &str = "assets_dir";

struct TmtoolsApp {
    modules: Vec<Box<dyn Module>>,
    active: usize,
    session: Session,
    screenshot: Option<screenshot::Screenshot>,
}

impl eframe::App for TmtoolsApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let wgpu = frame.wgpu_render_state().cloned();
        self.session.advance(ctx.input(|i| i.stable_dt) as f64);
        if self.session.playback.playing {
            ctx.request_repaint();
        }
        self.handle_dropped_files(&ctx);
        self.handle_keys(&ctx);

        egui::Panel::top("tmtools-tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("tmtools");
                ui.separator();
                for (i, m) in self.modules.iter().enumerate() {
                    let tab = ui.selectable_label(self.active == i, m.name());
                    if tab.on_hover_text(m.description()).clicked() {
                        self.active = i;
                    }
                }
            });
        });

        egui::Panel::bottom("tmtools-playback").show(ui, |ui| playback_bar(ui, &mut self.session));

        egui::Panel::left("tmtools-runs").default_size(270.0).resizable(true).show(ui, |ui| {
            egui::Panel::bottom("tmtools-assets").show(ui, |ui| assets_panel(ui, &mut self.session));
            runs_panel(ui, &mut self.session);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(m) = self.modules.get_mut(self.active) {
                m.ui(ui, &mut self.session, wgpu.as_ref());
            }
        });

        if let Some(shot) = &mut self.screenshot {
            shot.update(&ctx);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(store) = &self.session.assets {
            storage.set_string(ASSETS_KEY, store.root().display().to_string());
        }
    }
}

impl TmtoolsApp {
    fn open_assets(&mut self, dir: &std::path::Path) {
        if let Err(e) = self.session.open_assets(dir) {
            self.session.status = Some(e);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        for path in dropped {
            self.session.load_path(&path);
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |key| ctx.input(|i| i.key_pressed(key));
        if pressed(egui::Key::Space) {
            self.session.toggle_play();
        }
        if pressed(egui::Key::Comma) {
            self.session.step_ticks(-1);
        }
        if pressed(egui::Key::Period) {
            self.session.step_ticks(1);
        }
        if pressed(egui::Key::Home) {
            self.session.seek(0.0);
        }
    }
}

fn runs_panel(ui: &mut egui::Ui, session: &mut Session) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Open...").on_hover_text("Open replays or any Gbx file; you can also drop files on the window").clicked() {
            if let Some(paths) = rfd::FileDialog::new().add_filter("Gbx", &["gbx", "Gbx", "GBX"]).pick_files() {
                for path in paths {
                    session.load_path(&path);
                }
            }
        }
        if session.runs.len() > 1 && ui.button("Show all").clicked() {
            session.runs.iter_mut().for_each(|r| r.visible = true);
        }
    });
    if let Some(status) = &session.status {
        ui.label(egui::RichText::new(status).small().weak());
    }
    ui.separator();

    if session.files.is_empty() {
        ui.label("Drop .Replay.Gbx files here or use Open.");
        return;
    }

    let mut remove = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (f, file) in session.files.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("x").on_hover_text("Close this file").clicked() {
                    remove = Some(f);
                }
                let path = file.path.as_ref().map_or_else(|| file.name.clone(), |p| p.display().to_string());
                ui.label(egui::RichText::new(file.title()).strong()).on_hover_text(path);
            });
            // The environment and file name sit under the map name, smaller
            let detail = match &file.replay {
                Ok(replay) => format!("{}  {}", replay.map.environment.as_deref().unwrap_or(""), file.name),
                Err(_) => "Not a replay; open it in the Gbx Inspector".into(),
            };
            ui.label(egui::RichText::new(detail.trim()).small().weak());
            for r in 0..session.runs.len() {
                if session.runs[r].file != f {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.checkbox(&mut session.runs[r].visible, "").on_hover_text("Show this run");
                    color_swatch(ui, session.runs[r].color);
                    let focused = session.focus == Some(r);
                    if ui.selectable_label(focused, &session.runs[r].label).on_hover_text("Focus this run").clicked() {
                        session.focus = Some(r);
                        session.runs[r].visible = true;
                    }
                });
            }
            ui.add_space(6.0);
        }
    });
    if let Some(f) = remove {
        session.remove_file(f);
    }
}

/// The asset export setting, at the foot of the side panel
fn assets_panel(ui: &mut egui::Ui, session: &mut Session) {
    ui.separator();
    ui.label(egui::RichText::new("Game assets").strong());
    match &session.assets {
        Some(store) => {
            ui.label(egui::RichText::new(store.root().display().to_string()).small().weak());
        }
        None => {
            ui.label(egui::RichText::new("Not set. Choose a gbx-tools-3d asset export to draw maps with real meshes and textures.").small().weak());
        }
    }
    if ui.button("Choose asset folder...").clicked() {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            if let Err(e) = session.open_assets(&dir) {
                session.status = Some(e);
            }
        }
    }
}

fn playback_bar(ui: &mut egui::Ui, session: &mut Session) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let label = if session.playback.playing { "Pause" } else { "Play" };
        if ui.button(label).on_hover_text("Space").clicked() {
            session.toggle_play();
        }
        if ui.button("<").on_hover_text("Back one tick (,)").clicked() {
            session.step_ticks(-1);
        }
        if ui.button(">").on_hover_text("Forward one tick (.)").clicked() {
            session.step_ticks(1);
        }

        let duration = session.duration_ms();
        ui.monospace(format!("{} / {}", format_race_time(session.tick_ms() as i64), format_race_time(duration as i64)));
        ui.label(egui::RichText::new(format!("tick {}", session.tick_ms() / 10)).weak());

        egui::ComboBox::from_id_salt("speed").width(70.0).selected_text(format!("{}x", session.playback.speed)).show_ui(ui, |ui| {
            for speed in SPEEDS {
                ui.selectable_value(&mut session.playback.speed, speed, format!("{speed}x"));
            }
        });

        let mut time = session.playback.time_ms;
        ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(80.0);
        let slider = egui::Slider::new(&mut time, 0.0..=duration.max(1.0)).show_value(false);
        if ui.add(slider).changed() {
            session.seek(time);
        }
    });
    ui.add_space(4.0);
}

/// Command line: replay or Gbx paths, plus --tab NAME, --time MS, --focus N, and --assets DIR for opening straight to a view
fn apply_args(app: &mut TmtoolsApp) {
    let mut args = std::env::args().skip(1);
    let mut time = None;
    let mut focus = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tab" => {
                if let Some(name) = args.next() {
                    let wanted = name.to_lowercase();
                    app.active = app.modules.iter().position(|m| m.name().to_lowercase().contains(&wanted)).unwrap_or(app.active);
                }
            }
            "--time" => time = args.next().and_then(|t| t.parse::<f64>().ok()),
            "--focus" => focus = args.next().and_then(|f| f.parse::<usize>().ok()),
            "--assets" => {
                if let Some(dir) = args.next() {
                    app.open_assets(std::path::Path::new(&dir));
                }
            }
            path => app.session.load_path(std::path::Path::new(path)),
        }
    }
    if let Some(f) = focus.filter(|&f| f < app.session.runs.len()) {
        app.session.focus = Some(f);
    }
    if let Some(t) = time {
        app.session.seek(t);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 860.0]).with_title("tmtools").with_drag_and_drop(true),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "tmtools",
        options,
        Box::new(|cc| {
            fonts::add_unicode_fallback(&cc.egui_ctx);
            let modules: Vec<Box<dyn Module>> = vec![
                Box::new(inputscope::InputScopeModule::default()),
                Box::new(telemetry::TelemetryModule::default()),
                Box::new(trajmap::TrajMapModule::default()),
                Box::new(runreport::RunReportModule::default()),
                Box::new(replayview::ReplayViewModule::default()),
                Box::new(gbxinspect::GbxInspectModule::default()),
            ];
            let mut app = TmtoolsApp { modules, active: 0, session: Session::default(), screenshot: screenshot::Screenshot::from_env() };
            // The asset folder comes from the environment, else the one chosen last time; --assets overrides both
            let remembered = std::env::var_os("TMTOOLS_ASSETS_DIR")
                .map(PathBuf::from)
                .or_else(|| cc.storage.and_then(|s| s.get_string(ASSETS_KEY)).map(PathBuf::from));
            if let Some(dir) = remembered {
                app.open_assets(&dir);
            }
            apply_args(&mut app);
            Ok(Box::new(app))
        }),
    )
}
