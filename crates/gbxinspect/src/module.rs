// ABOUTME: GbxInspectModule UI: pick a loaded file or a replay's embedded map, see its header and chunk tree, and
// ABOUTME: select a header chunk, a body chunk, or the whole body to view its bytes in hex.

use crate::hex::{chunk_label, hex_row, ROW_BYTES};
use egui::{Color32, RichText};
use gbx::class_ids::class_name;
use gbx::GbxFile;
use std::sync::Arc;
use tmreplay::{trace_body, BodyTrace};
use tmtools_core::{Module, Session};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Header(usize),
    Chunk(usize),
    Body,
}

struct Inspection {
    /// Which bytes this was built from: the file's allocation and whether it is the embedded map
    source: (usize, bool),
    file: Result<GbxFile, String>,
    trace: BodyTrace,
}

#[derive(Default)]
pub struct GbxInspectModule {
    file: usize,
    embedded: bool,
    inspection: Option<Inspection>,
    selection: Option<Selection>,
}

impl Module for GbxInspectModule {
    fn name(&self) -> &str {
        "Gbx Inspector"
    }

    fn description(&self) -> &str {
        "Header, chunk tree, and hex view of any loaded Gbx file or a replay's embedded map"
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&tmtools_core::RenderState>) {
        if session.files.is_empty() {
            ui.centered_and_justified(|ui| ui.label("Open any .Gbx file to inspect it."));
            return;
        }
        self.file = self.file.min(session.files.len() - 1);

        ui.horizontal(|ui| {
            ui.label("File");
            // Map names first; the file name tells apart replays of the same map
            let label = |f: &tmtools_core::LoadedFile| if f.title() == f.name { f.name.clone() } else { format!("{} ({})", f.title(), f.name) };
            egui::ComboBox::from_id_salt("gbxinspect-file").selected_text(label(&session.files[self.file])).show_ui(ui, |ui| {
                for (i, f) in session.files.iter().enumerate() {
                    ui.selectable_value(&mut self.file, i, label(f));
                }
            });
            let has_map = session.files[self.file].replay.as_ref().is_ok_and(|r| r.embedded_map.is_some());
            if !has_map {
                self.embedded = false;
            }
            ui.add_enabled(has_map, egui::Checkbox::new(&mut self.embedded, "Embedded map"));
        });
        ui.separator();

        let file = &session.files[self.file];
        let source = (Arc::as_ptr(&file.bytes) as usize, self.embedded);
        if self.inspection.as_ref().is_none_or(|i| i.source != source) {
            let bytes: &[u8] = if self.embedded {
                file.replay.as_ref().ok().and_then(|r| r.embedded_map.as_deref()).unwrap_or(&[])
            } else {
                &file.bytes
            };
            let parsed = GbxFile::parse(bytes).map_err(|e| e.to_string());
            let trace = parsed.as_ref().map(trace_body).unwrap_or_default();
            self.inspection = Some(Inspection { source, file: parsed, trace });
            self.selection = None;
        }
        let inspection = self.inspection.as_ref().expect("built above");
        let gbx = match &inspection.file {
            Ok(gbx) => gbx,
            Err(e) => {
                ui.colored_label(Color32::LIGHT_RED, format!("Not a readable Gbx file: {e}"));
                return;
            }
        };

        let mut selection = self.selection;
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical().id_salt("gbxinspect-structure").auto_shrink(false).show(&mut cols[0], |ui| {
                summary(ui, gbx);
                header_chunks(ui, gbx, &mut selection);
                externals(ui, gbx);
                body_chunks(ui, gbx, &inspection.trace, &mut selection);
            });
            hex_view(&mut cols[1], gbx, &inspection.trace, selection);
        });
        self.selection = selection;
    }
}

fn summary(ui: &mut egui::Ui, gbx: &GbxFile) {
    ui.heading("Header");
    egui::Grid::new("gbxinspect-summary").num_columns(2).striped(true).show(ui, |ui| {
        let class = class_name(gbx.class_id).unwrap_or("unknown class");
        let rows = [
            ("Version", gbx.version.to_string()),
            ("Class", format!("{class} (0x{:08X})", gbx.class_id)),
            ("Stored class ID", format!("0x{:08X}", gbx.raw_class_id)),
            ("Header chunks", gbx.header_chunks.len().to_string()),
            ("Nodes", gbx.node_count.to_string()),
            ("External references", gbx.external.len().to_string()),
            ("Body", format!("{} bytes at file offset {}", gbx.body.len(), gbx.body_offset)),
            (
                "Body compression",
                gbx.body_compressed_size.map_or("none".into(), |c| format!("LZO, {c} bytes ({:.0}%)", c as f64 * 100.0 / gbx.body.len().max(1) as f64)),
            ),
        ];
        for (k, v) in rows {
            ui.label(k);
            ui.label(v);
            ui.end_row();
        }
    });
    ui.add_space(8.0);
}

fn header_chunks(ui: &mut egui::Ui, gbx: &GbxFile, selection: &mut Option<Selection>) {
    egui::CollapsingHeader::new(format!("Header chunks ({})", gbx.header_chunks.len())).default_open(true).show(ui, |ui| {
        egui::Grid::new("gbxinspect-header-chunks").num_columns(3).striped(true).show(ui, |ui| {
            for (i, chunk) in gbx.header_chunks.iter().enumerate() {
                let label = format!("{}{}", chunk_label(chunk.id), if chunk.heavy { "  heavy" } else { "" });
                if ui.selectable_label(*selection == Some(Selection::Header(i)), label).clicked() {
                    *selection = Some(Selection::Header(i));
                }
                ui.monospace(format!("0x{:08X}", chunk.id));
                ui.label(format!("{} bytes", chunk.data.len()));
                ui.end_row();
            }
        });
    });
}

fn externals(ui: &mut egui::Ui, gbx: &GbxFile) {
    if gbx.external.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("External references ({})", gbx.external.len())).show(ui, |ui| {
        for ext in &gbx.external {
            ui.label(format!("node {}: {}", ext.node_index, ext.name));
        }
    });
}

fn body_chunks(ui: &mut egui::Ui, gbx: &GbxFile, trace: &BodyTrace, selection: &mut Option<Selection>) {
    egui::CollapsingHeader::new(format!("Body chunks ({})", trace.spans.len())).default_open(true).show(ui, |ui| {
        if ui.selectable_label(*selection == Some(Selection::Body), format!("Whole body ({} bytes)", gbx.body.len())).clicked() {
            *selection = Some(Selection::Body);
        }
        egui::Grid::new("gbxinspect-body-chunks").num_columns(5).striped(true).show(ui, |ui| {
            for (i, span) in trace.spans.iter().enumerate() {
                let indent = "    ".repeat(span.depth as usize);
                let mut label = RichText::new(format!("{indent}{}", chunk_label(span.id)));
                if !span.read {
                    label = label.weak();
                }
                if ui.selectable_label(*selection == Some(Selection::Chunk(i)), label).clicked() {
                    *selection = Some(Selection::Chunk(i));
                }
                ui.monospace(format!("0x{:08X}", span.id));
                ui.monospace(format!("@{}", span.start));
                ui.label(format!("{} bytes", span.end - span.data_start));
                let kind = match (span.skippable, span.read) {
                    (true, true) => "skippable, read",
                    (true, false) => "skippable, skipped",
                    (false, _) => "read",
                };
                ui.label(RichText::new(kind).small());
                ui.end_row();
            }
        });
        if let Some(note) = &trace.note {
            let at = trace.stopped_at.map_or(String::new(), |p| format!(" at byte {p} of {}", gbx.body.len()));
            ui.add_space(4.0);
            ui.colored_label(Color32::from_rgb(230, 190, 90), format!("Walk ended{at}: {note}"));
        }
    });
}

fn hex_view(ui: &mut egui::Ui, gbx: &GbxFile, trace: &BodyTrace, selection: Option<Selection>) {
    let (title, bytes, base): (String, &[u8], usize) = match selection {
        Some(Selection::Header(i)) => {
            let c = &gbx.header_chunks[i];
            (format!("Header chunk {}", chunk_label(c.id)), &c.data, 0)
        }
        Some(Selection::Chunk(i)) => {
            let s = &trace.spans[i];
            (format!("{} data, body bytes {}..{}", chunk_label(s.id), s.data_start, s.end), &gbx.body[s.data_start..s.end], s.data_start)
        }
        Some(Selection::Body) => ("Whole body".into(), &gbx.body, 0),
        None => {
            ui.label("Select a header chunk, a body chunk, or the whole body to see its bytes.");
            return;
        }
    };
    ui.heading(title);
    ui.label(RichText::new(format!("{} bytes; offsets are into the {}", bytes.len(), if matches!(selection, Some(Selection::Header(_))) { "chunk" } else { "body" })).small().weak());
    let rows = bytes.len().div_ceil(ROW_BYTES);
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical().id_salt("gbxinspect-hex").auto_shrink(false).show_rows(ui, row_height, rows, |ui, range| {
        for row in range {
            let start = row * ROW_BYTES;
            let end = (start + ROW_BYTES).min(bytes.len());
            ui.monospace(hex_row(base + start, &bytes[start..end]));
        }
    });
}
