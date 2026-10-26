// ABOUTME: The contract between the tmtools shell and its modules, and the Session every module reads and drives.
// ABOUTME: One clock is shared, so seeking in any module moves all of them.

mod session;

pub use eframe::egui_wgpu::RenderState;
pub use session::{LoadedFile, Playback, Run, Session, SPEEDS};

/// A self-contained analysis tool hosted by the tmtools shell.
///
/// The shell owns the window, the tab bar, the file and run list, and the playback bar. Each module draws itself
/// into the central panel from the shared [`Session`], and may seek or change the focused run through it. `wgpu`
/// is the window's GPU state, for modules that render with their own pipelines; others ignore it.
pub trait Module {
    /// Stable, human-readable name shown in the tab bar
    fn name(&self) -> &str;

    /// One-line description shown as the tab's tooltip
    fn description(&self) -> &str {
        ""
    }

    /// Draw the module UI for this frame
    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, wgpu: Option<&RenderState>);
}

/// A small filled square in the run's color, for legends and lists
pub fn color_swatch(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}
