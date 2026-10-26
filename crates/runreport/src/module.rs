// ABOUTME: RunReportModule UI: the sector table (each run's sector times, the best of each sector, the theoretical
// ABOUTME: best) for the focused run's map, with splits from checkpoints and points placed on the Trajectory Map.

use egui::{Color32, RichText};
use tmreplay::format_race_time;
use tmreplay::sectors::{best_sectors, boundaries, boundary_times, sector_durations, Boundary};
use tmtools_core::{color_swatch, Module, RenderState, Session};

const BEST: Color32 = Color32::from_rgb(90, 220, 120);
const LOSS: Color32 = Color32::from_rgb(240, 120, 110);

/// A sector length in seconds with hundredths, as race times show it
fn seconds(ms: f32) -> String {
    format!("{:.2}", (ms / 10.0).floor() / 100.0)
}

/// "+0.12" for time lost against the best
fn loss(ms: f32) -> String {
    format!("+{:.2}", (ms / 10.0).round() / 100.0)
}

#[derive(Default)]
pub struct RunReportModule;

impl Module for RunReportModule {
    fn name(&self) -> &str {
        "Run Report"
    }

    fn description(&self) -> &str {
        "Sector times with the theoretical best for the runs on the focused run's map"
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&RenderState>) {
        let Some(focus) = session.focus else {
            ui.centered_and_justified(|ui| ui.label("Open a replay to see its report."));
            return;
        };
        let seek = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| sector_table(ui, session, focus)).inner;
        if let Some(t) = seek {
            session.playback.playing = false;
            session.seek(t as f64);
        }
    }
}

/// The sector table; returns a time to seek to when a cell is clicked
fn sector_table(ui: &mut egui::Ui, session: &Session, focus: usize) -> Option<f32> {
    ui.heading("Sectors");
    let runs = session.runs_on_map_of(focus);
    let splits = session.splits_for(focus);
    let bounds = boundaries(session.ghost(focus), splits);
    if bounds.is_empty() {
        ui.label("The focused run has no checkpoints or splits to divide it into sectors.");
        return None;
    }
    ui.label(
        RichText::new(format!(
            "{} sector{} from checkpoints{}. Place more splits with Place splits on the Trajectory Map. Click a time to jump to the start of that sector.",
            bounds.len(),
            if bounds.len() == 1 { "" } else { "s" },
            if splits.is_empty() { String::new() } else { format!(" and {} placed splits", splits.len()) }
        ))
        .small()
        .weak(),
    );

    let times: Vec<Vec<Option<f32>>> = runs.iter().map(|&r| boundary_times(session.ghost(r), &bounds)).collect();
    let sectors: Vec<Vec<Option<f32>>> = times.iter().map(|t| sector_durations(t)).collect();
    let (best, theoretical) = best_sectors(&sectors);

    let mut seek = None;
    egui::Grid::new("runreport-sectors").striped(true).num_columns(bounds.len() + 2).show(ui, |ui| {
        ui.strong("Run");
        for (i, b) in bounds.iter().enumerate() {
            let ends = match b {
                Boundary::Checkpoint(c) if *c + 1 == session.ghost(focus).checkpoints.len() => "the finish".to_owned(),
                Boundary::Checkpoint(c) => format!("checkpoint {}", c + 1),
                Boundary::Point(_) => "a placed split".to_owned(),
            };
            ui.strong(format!("S{}", i + 1)).on_hover_text(format!("Ends at {ends}"));
        }
        ui.strong("Total");
        ui.end_row();

        for (row, &run) in runs.iter().enumerate() {
            ui.horizontal(|ui| {
                color_swatch(ui, session.runs[run].color);
                let label = RichText::new(&session.runs[run].label);
                ui.label(if run == focus { label.strong() } else { label });
            });
            for (i, sector) in sectors[row].iter().enumerate() {
                match (sector, best[i]) {
                    (Some(t), Some(b)) => {
                        let lost = t - b;
                        let text = if lost < 5.0 {
                            RichText::new(seconds(*t)).color(BEST).strong()
                        } else {
                            RichText::new(format!("{}  {}", seconds(*t), loss(lost))).color(LOSS)
                        };
                        let start = if i == 0 { Some(0.0) } else { times[row][i - 1] };
                        if ui.link(text).on_hover_text("Jump to the start of this sector for this run").clicked() {
                            seek = start;
                        }
                    }
                    _ => {
                        ui.label(RichText::new("-").weak());
                    }
                }
            }
            let total = session.ghost(run).race_time_ms.map_or("-".into(), |t| format_race_time(t as i64));
            ui.label(total);
            ui.end_row();
        }

        ui.label(RichText::new("Best").strong());
        for b in &best {
            ui.label(b.map_or("-".into(), seconds));
        }
        ui.label(theoretical.map_or("-".into(), |t| format_race_time(t.round() as i64)));
        ui.end_row();
    });

    if let (Some(theoretical), Some(actual)) = (theoretical, session.ghost(focus).race_time_ms) {
        let saving = actual as f32 - theoretical;
        ui.add_space(6.0);
        ui.label(format!(
            "Theoretical best {}: the best of each sector across {} run{}. The focused run is {} off it.",
            format_race_time(theoretical.round() as i64),
            runs.len(),
            if runs.len() == 1 { "" } else { "s" },
            seconds(saving.max(0.0))
        ));
    }
    seek
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sector_times_and_losses() {
        assert_eq!(seconds(12_857.0), "12.85");
        assert_eq!(loss(124.0), "+0.12");
    }
}
