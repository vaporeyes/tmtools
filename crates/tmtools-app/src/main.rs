// ABOUTME: tmtools application shell: an eframe window with a module tab bar, the loaded files and runs, and the
// ABOUTME: playback bar every module shares. Register additional modules by pushing them into the registry in main.

mod fonts;
mod library;
mod screenshot;

use eframe::egui;
use std::path::PathBuf;
use tmreplay::format_race_time;
use tmtools_core::theme::{self, FINISH, INK, INK_RAISED, INK_SOFT, TEXT, WEAK};
use tmtools_core::{color_swatch, icon_button, section, Icon, Module, SavedSession, Session, SPEEDS};

// Where the chosen asset folder and the last session are remembered between launches
const ASSETS_KEY: &str = "assets_dir";
const SESSION_KEY: &str = "session";

struct TmtoolsApp {
    modules: Vec<Box<dyn Module>>,
    active: usize,
    session: Session,
    screenshot: Option<screenshot::Screenshot>,
    library: library::LibraryWindow,
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

        egui::Panel::top("tmtools-tabs").frame(egui::Frame::new().fill(theme::NIGHT).inner_margin(egui::Margin::symmetric(12, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("tmtools").size(18.0).strong().color(FINISH));
                ui.add_space(12.0);
                for (i, m) in self.modules.iter().enumerate() {
                    let tab = ui.selectable_label(self.active == i, egui::RichText::new(m.name()).size(14.0));
                    if tab.on_hover_text(m.description()).clicked() {
                        self.active = i;
                    }
                }
            });
        });

        egui::Panel::bottom("tmtools-playback")
            .frame(egui::Frame::new().fill(theme::NIGHT).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| playback_bar(ui, &mut self.session));

        egui::Panel::left("tmtools-runs").default_size(270.0).resizable(true).show(ui, |ui| {
            egui::Panel::bottom("tmtools-assets").show(ui, |ui| assets_panel(ui, &mut self.session));
            if runs_panel(ui, &mut self.session) {
                self.library.open = true;
                self.library.refresh();
            }
        });
        self.library.ui(&ctx, &mut self.session);

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
        // Scripted screenshot runs must not replace the session someone was working in
        if self.screenshot.is_some() {
            return;
        }
        if let Some(store) = &self.session.assets {
            storage.set_string(ASSETS_KEY, store.root().display().to_string());
        }
        let mut saved = self.session.save();
        saved.active_tab = self.active;
        saved.settings.insert("shell.library_url".into(), self.library.url.clone());
        for m in &self.modules {
            m.save_settings(&mut saved.settings);
        }
        storage.set_string(SESSION_KEY, saved.to_json());
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

/// The loaded files and their runs; returns true when the replay library was asked for
fn runs_panel(ui: &mut egui::Ui, session: &mut Session) -> bool {
    let mut library = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        section(ui, "Files");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Library...").on_hover_text("Open replays from a gbx-tools-3d-node replay service").clicked() {
                library = true;
            }
            if ui.button("Open...").on_hover_text("Open replays or any Gbx file; you can also drop files on the window").clicked() {
                if let Some(paths) = rfd::FileDialog::new().add_filter("Gbx", &["gbx", "Gbx", "GBX"]).pick_files() {
                    for path in paths {
                        session.load_path(&path);
                    }
                }
            }
        });
    });
    if let Some(status) = &session.status {
        ui.label(egui::RichText::new(status).small().weak());
    }
    ui.add_space(4.0);

    if session.files.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| ui.label(egui::RichText::new("Drop .Replay.Gbx files here\nor use Open or Library.").color(WEAK)));
        return library;
    }

    if session.runs.len() > 1 {
        ui.horizontal(|ui| {
            if ui.small_button("Select all").on_hover_text("Show every run").clicked() {
                session.runs.iter_mut().for_each(|r| r.visible = true);
            }
            if ui.small_button("Select none").on_hover_text("Hide every run").clicked() {
                session.runs.iter_mut().for_each(|r| r.visible = false);
            }
        });
        ui.add_space(4.0);
    }

    let mut remove = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (f, file) in session.files.iter().enumerate() {
            // One card per file: its map, where it comes from, and its runs
            egui::Frame::new().fill(INK_RAISED).corner_radius(6).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let path = file.path.as_ref().map_or_else(|| file.name.clone(), |p| p.display().to_string());
                    ui.label(egui::RichText::new(file.title()).strong().color(TEXT)).on_hover_text(path);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, 18.0, false, "Close this file").clicked() {
                            remove = Some(f);
                        }
                    });
                });
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
                        ui.checkbox(&mut session.runs[r].visible, "").on_hover_text("Show this run");
                        color_swatch(ui, session.runs[r].color);
                        let focused = session.focus == Some(r);
                        if ui.selectable_label(focused, &session.runs[r].label).on_hover_text("Focus this run").clicked() {
                            session.focus = Some(r);
                            session.runs[r].visible = true;
                        }
                    });
                }
            });
            ui.add_space(6.0);
        }
    });
    if let Some(f) = remove {
        session.remove_file(f);
    }
    library
}

/// The asset export setting, at the foot of the side panel
fn assets_panel(ui: &mut egui::Ui, session: &mut Session) {
    ui.add_space(6.0);
    section(ui, "Game assets");
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
    ui.add_space(4.0);
}

fn playback_bar(ui: &mut egui::Ui, session: &mut Session) {
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::ToStart, 26.0, false, "Back to the start (Home)").clicked() {
            session.seek(0.0);
        }
        if icon_button(ui, Icon::StepBack, 26.0, false, "Back one tick (,)").clicked() {
            session.step_ticks(-1);
        }
        let (icon, tip) = if session.playback.playing { (Icon::Pause, "Pause (Space)") } else { (Icon::Play, "Play (Space)") };
        if icon_button(ui, icon, 36.0, true, tip).clicked() {
            session.toggle_play();
        }
        if icon_button(ui, Icon::StepForward, 26.0, false, "Forward one tick (.)").clicked() {
            session.step_ticks(1);
        }
        ui.add_space(8.0);

        let duration = session.duration_ms();
        ui.label(egui::RichText::new(format_race_time(session.tick_ms() as i64)).monospace().size(20.0).color(TEXT));
        ui.label(egui::RichText::new(format!("/ {}", format_race_time(duration as i64))).monospace().color(WEAK));
        ui.label(egui::RichText::new(format!("tick {}", session.tick_ms() / 10)).small().color(WEAK));
        ui.add_space(8.0);

        egui::ComboBox::from_id_salt("speed").width(64.0).selected_text(format!("{}x", session.playback.speed)).show_ui(ui, |ui| {
            for speed in SPEEDS {
                ui.selectable_value(&mut session.playback.speed, speed, format!("{speed}x"));
            }
        });
        ui.add_space(8.0);
        timeline(ui, session, duration);
    });
}

/// The scrubber: progress in yellow, the focused run's checkpoints as marks; click or drag to seek
fn timeline(ui: &mut egui::Ui, session: &mut Session, duration: f64) {
    let width = ui.available_width().max(80.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect.expand(8.0));
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 12.0, 6.0));
    let x_of = |ms: f64| track.left() + (ms / duration.max(1.0)) as f32 * track.width();

    painter.rect_filled(track, 3.0, INK_SOFT);
    let done = egui::Rect::from_min_max(track.min, egui::pos2(x_of(session.playback.time_ms), track.max.y));
    painter.rect_filled(done, 3.0, FINISH.gamma_multiply(0.8));
    if let Some(focus) = session.focus {
        for t in session.ghost(focus).checkpoints.iter().filter_map(|c| c.time_ms) {
            let x = x_of(t as f64);
            painter.line_segment([egui::pos2(x, track.top() - 6.0), egui::pos2(x, track.bottom() + 6.0)], egui::Stroke::new(2.0, TEXT));
        }
    }
    painter.circle(egui::pos2(x_of(session.playback.time_ms), track.center().y), 8.0, FINISH, egui::Stroke::new(2.0, INK));

    if let Some(pos) = response.interact_pointer_pos().filter(|_| response.clicked() || response.dragged()) {
        let fraction = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
        session.seek(fraction * duration);
    }
    if let Some(pos) = response.hover_pos() {
        let fraction = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
        response.on_hover_text_at_pointer(format_race_time((fraction * duration) as i64));
    }
}

/// What the command line asks for: replay or Gbx paths, plus --tab NAME, --time MS, --focus N, --assets DIR, and
/// --library (open the replay library window)
#[derive(Default)]
struct Args {
    paths: Vec<PathBuf>,
    tab: Option<String>,
    time: Option<f64>,
    focus: Option<usize>,
    assets: Option<PathBuf>,
    library: bool,
}

fn parse_args() -> Args {
    let mut parsed = Args::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tab" => parsed.tab = args.next(),
            "--time" => parsed.time = args.next().and_then(|t| t.parse::<f64>().ok()),
            "--focus" => parsed.focus = args.next().and_then(|f| f.parse::<usize>().ok()),
            "--assets" => parsed.assets = args.next().map(PathBuf::from),
            "--library" => parsed.library = true,
            path => parsed.paths.push(PathBuf::from(path)),
        }
    }
    parsed
}

/// The last session first, then the command line, which wins: files given there replace the remembered ones
fn start_session(app: &mut TmtoolsApp, saved: Option<SavedSession>, args: Args) {
    if let Some(saved) = &saved {
        for m in &mut app.modules {
            m.load_settings(&saved.settings);
        }
        app.active = saved.active_tab.min(app.modules.len().saturating_sub(1));
        if let Some(url) = saved.settings.get("shell.library_url") {
            app.library.url = url.clone();
        }
        app.session.restore(saved, args.paths.is_empty());
    }
    if let Some(dir) = &args.assets {
        app.open_assets(dir);
    }
    for path in &args.paths {
        app.session.load_path(path);
    }
    if let Some(name) = &args.tab {
        let wanted = name.to_lowercase();
        app.active = app.modules.iter().position(|m| m.name().to_lowercase().contains(&wanted)).unwrap_or(app.active);
    }
    if let Some(f) = args.focus.filter(|&f| f < app.session.runs.len()) {
        app.session.focus = Some(f);
    }
    if let Some(t) = args.time {
        app.session.seek(t);
    }
    if args.library {
        app.library.open = true;
        app.library.refresh();
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
            theme::apply(&cc.egui_ctx);
            let modules: Vec<Box<dyn Module>> = vec![
                Box::new(inputscope::InputScopeModule::default()),
                Box::new(telemetry::TelemetryModule::default()),
                Box::new(trajmap::TrajMapModule::default()),
                Box::new(runreport::RunReportModule::default()),
                Box::new(replayview::ReplayViewModule::default()),
                Box::new(gbxinspect::GbxInspectModule::default()),
            ];
            let mut app = TmtoolsApp { modules, active: 0, session: Session::default(), screenshot: screenshot::Screenshot::from_env(), library: library::LibraryWindow::default() };
            // The asset folder comes from the environment, else the one chosen last time; --assets overrides both
            let remembered = std::env::var_os("TMTOOLS_ASSETS_DIR")
                .map(PathBuf::from)
                .or_else(|| cc.storage.and_then(|s| s.get_string(ASSETS_KEY)).map(PathBuf::from));
            if let Some(dir) = remembered {
                app.open_assets(&dir);
            }
            // Screenshot runs start clean, so they show exactly what the command line asks for
            let saved = if app.screenshot.is_some() { None } else { cc.storage.and_then(|s| s.get_string(SESSION_KEY)).and_then(|j| SavedSession::from_json(&j)) };
            start_session(&mut app, saved, parse_args());
            Ok(Box::new(app))
        }),
    )
}
