// ABOUTME: MapEditModule UI: a top-down plan of one level of the map with a block palette. Click to select or place,
// ABOUTME: R to rotate, arrows and Shift+Page Up/Down to move, Delete to remove; Save as writes a map the game can load.

use crate::doc::{Catalog, Cell, Doc, Kind};
use egui::{Color32, Key, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use std::sync::Arc;
use tmedit::Map;
use tmtools_core::theme::{FINISH, INK, INK_RAISED, TEXT, WEAK};
use tmtools_core::{section, setting, Module, Session, Settings};

const SIDE_WIDTH: f32 = 270.0;
const DIRECTIONS: [&str; 4] = ["North", "East", "South", "West"];
/// Screen direction of each block direction: north is +Z, drawn downward like the Trajectory Map
const POINTING: [(f32, f32); 4] = [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];

/// The plan's view: grid cell (x, z) at the center, and screen pixels per cell
#[derive(Debug, Clone, Copy)]
struct View {
    center: [f32; 2],
    scale: f32,
}

impl View {
    fn to_screen(self, x: f32, z: f32, screen_center: Pos2) -> Pos2 {
        screen_center + Vec2::new(x - self.center[0], z - self.center[1]) * self.scale
    }

    fn cell_at(self, screen: Pos2, screen_center: Pos2) -> (i32, i32) {
        let d = (screen - screen_center) / self.scale;
        ((self.center[0] + d.x).floor() as i32, (self.center[1] + d.y).floor() as i32)
    }

    fn cell_rect(self, x: i32, z: i32, screen_center: Pos2) -> Rect {
        Rect::from_two_pos(self.to_screen(x as f32, z as f32, screen_center), self.to_screen(x as f32 + 1.0, z as f32 + 1.0, screen_center))
    }
}

pub struct MapEditModule {
    doc: Option<Doc>,
    /// The palette block being placed; None means clicks select blocks
    brush: Option<String>,
    brush_dir: u8,
    level: i32,
    show_other_levels: bool,
    search: String,
    /// Login written as the author of saved maps, so the game lets that player edit them for free
    author: String,
    view: View,
    fitted: bool,
    message: Option<String>,
}

impl Default for MapEditModule {
    fn default() -> Self {
        Self {
            doc: None,
            brush: None,
            brush_dir: 0,
            level: 1,
            show_other_levels: true,
            search: String::new(),
            author: String::new(),
            view: View { center: [16.0, 16.0], scale: 16.0 },
            fitted: false,
            message: None,
        }
    }
}

fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Terrain => Color32::from_rgb(46, 70, 52),
        Kind::Road => Color32::from_rgb(200, 140, 60),
        Kind::Start => Color32::from_rgb(60, 190, 100),
        Kind::Finish => Color32::from_rgb(220, 70, 70),
        Kind::Checkpoint => Color32::from_rgb(70, 150, 240),
        Kind::Other => Color32::from_rgb(96, 112, 150),
    }
}

/// A seed for fresh UIDs
fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

/// A map from a map file or a replay's embedded map
fn open_bytes(bytes: &[u8], source: String) -> Result<Doc, String> {
    let map_bytes = match tmreplay::parse_replay(bytes) {
        Ok(replay) => replay.embedded_map.ok_or("This replay has no embedded map")?,
        Err(_) => bytes.to_vec(),
    };
    let map = Map::parse(&map_bytes).map_err(|e| format!("Not a map tmtools can edit: {e}"))?;
    let challenge = tmreplay::parse_challenge(&map_bytes).map_err(|e| e.to_string())?;
    let mut doc = Doc::new(map, source, seed());
    doc.collection = challenge.collection;
    Ok(doc)
}

/// A file name for a map name: formatting codes stripped and characters Windows forbids removed
fn file_name(name: &str) -> String {
    let plain: String = tmreplay::strip_formatting(name).chars().filter(|c| !r#"<>:"/\|?*"#.contains(*c)).collect();
    let plain = plain.trim();
    format!("{}.Challenge.Gbx", if plain.is_empty() { "tmtools map" } else { plain })
}

impl MapEditModule {
    fn open(&mut self, doc: Doc) {
        // Start on the level most blocks are on, where the track usually is
        let mut counts = std::collections::HashMap::new();
        for b in doc.blocks().blocks.iter().filter(|b| !b.is_clip()) {
            *counts.entry(b.stored[1] as i32).or_insert(0) += 1;
        }
        self.level = counts.into_iter().max_by_key(|&(y, n)| (n, -y)).map_or(1, |(y, _)| y);
        self.message = Some(format!("Opened {}", doc.source));
        self.doc = Some(doc);
        self.fitted = false;
        self.brush = None;
    }

    /// Loads the block catalog for the open map's environment once the game assets are available
    fn ensure_catalog(&mut self, session: &Session) {
        let Some(doc) = self.doc.as_mut() else { return };
        if doc.catalog.is_some() {
            return;
        }
        let Some(store) = &session.assets else { return };
        let Ok(infos) = store.block_infos(&doc.collection) else { return };
        // The first level above the default terrain, as the 3D view places it
        let size = doc.blocks().size;
        let base = store.decorations(&doc.collection).ok().and_then(|d| d.iter().find(|d| d.size == size).map(|d| d.base_height()));
        let ground = base.map(|b| b + 1).or_else(|| doc.blocks().blocks.iter().filter(|b| b.is_ground()).map(|b| b.stored[1] as i32).min()).unwrap_or(1);
        doc.catalog = Some(Arc::new(Catalog::new(&infos, ground)));
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, session: &Session) {
        tmtools_core::toolbar(ui, |ui| {
            if ui.button("Open map...").on_hover_text("Open a .Challenge.Gbx map, or a replay to edit its map").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Gbx", &["gbx", "Gbx", "GBX"]).pick_file() {
                    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| open_bytes(&b, path.display().to_string())) {
                        Ok(doc) => self.open(doc),
                        Err(e) => self.message = Some(e),
                    }
                }
            }
            let maps: Vec<usize> = (0..session.files.len()).filter(|&f| session.files[f].replay.as_ref().map_or(true, |r| r.embedded_map.is_some())).collect();
            if !maps.is_empty() {
                let mut chosen = None;
                egui::ComboBox::from_id_salt("mapedit-loaded").selected_text("Edit a loaded map").show_ui(ui, |ui| {
                    for &f in &maps {
                        if ui.selectable_label(false, session.files[f].title()).clicked() {
                            chosen = Some(f);
                        }
                    }
                });
                if let Some(f) = chosen {
                    let file = &session.files[f];
                    match open_bytes(&file.bytes, file.title()) {
                        Ok(doc) => self.open(doc),
                        Err(e) => self.message = Some(e),
                    }
                }
            }
            let Some(doc) = self.doc.as_mut() else { return };
            ui.separator();
            if ui.add_enabled(doc.can_undo(), egui::Button::new("Undo")).on_hover_text("Ctrl+Z").clicked() {
                doc.undo();
            }
            if ui.add_enabled(doc.can_redo(), egui::Button::new("Redo")).on_hover_text("Ctrl+Shift+Z").clicked() {
                doc.redo();
            }
            ui.separator();
            if ui.button("-").on_hover_text("Level down (Page Down)").clicked() {
                self.level = (self.level - 1).max(0);
            }
            ui.label(egui::RichText::new(format!("Level {}", self.level)).monospace());
            if ui.button("+").on_hover_text("Level up (Page Up)").clicked() {
                self.level = (self.level + 1).min(doc.blocks().size[1] - 1);
            }
            ui.checkbox(&mut self.show_other_levels, "Show levels below");
            if ui.button("Fit").clicked() {
                self.fitted = false;
            }
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, session: &Session) {
        let Some(doc) = self.doc.as_mut() else { return };
        section(ui, "Map");
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut doc.name);
        });
        ui.horizontal(|ui| {
            ui.label("Author");
            ui.add(egui::TextEdit::singleline(&mut self.author).hint_text("your TMF login"))
                .on_hover_text("Saved maps name this login as their author, so you can open them in the game's editor without paying coppers");
        });
        let s = doc.blocks().size;
        ui.label(egui::RichText::new(format!("{}  {}x{}x{}  {} blocks", doc.collection, s[0], s[1], s[2], doc.blocks().blocks.len())).small().weak());
        if ui.button("Save as...").clicked() {
            let dialog = rfd::FileDialog::new().add_filter("TMF map", &["Gbx"]).set_file_name(file_name(&doc.name));
            if let Some(path) = dialog.save_file() {
                self.message = Some(match doc.to_bytes(self.author.trim()).and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())) {
                    Ok(()) => {
                        doc.dirty = false;
                        format!("Saved {} (UID {})", path.display(), doc.uid)
                    }
                    Err(e) => format!("Not saved: {e}"),
                });
            }
        }
        if !doc.map.tail.ids_free() {
            ui.add_space(4.0);
            if doc.map.tail.clip_spans.is_empty() {
                ui.label(egui::RichText::new("This map has data after its blocks that tmtools can't read, so only block types it already uses can be placed.").small().color(WEAK));
            } else {
                ui.label(egui::RichText::new("MediaTracker clips may refer to block names, so only block types the map already uses can be placed.").small().color(WEAK));
                if ui.button("Drop clips").on_hover_text("Remove the intro, in-game, end-race, and global clips").clicked() {
                    doc.drop_clips();
                }
            }
        }

        ui.add_space(8.0);
        section(ui, "Selection");
        match doc.selected {
            Some(i) => {
                let b = doc.map_block(i);
                ui.label(egui::RichText::new(&b.name).color(TEXT));
                ui.label(egui::RichText::new(format!("cell {}, {}, {}  facing {}{}", b.x, b.y, b.z, DIRECTIONS[b.dir as usize], if b.is_ground { "  on ground" } else { "" })).small().weak());
                ui.horizontal(|ui| {
                    if ui.button("Rotate").on_hover_text("R").clicked() {
                        if let Err(e) = doc.transform(i, (0, 0, 0), true) {
                            self.message = Some(e);
                        }
                    }
                    if ui.button("Delete").on_hover_text("Delete").clicked() {
                        doc.delete(i);
                    }
                });
            }
            None => {
                ui.label(egui::RichText::new("Click a block on this level to select it. Arrows move it, Shift+Page Up/Down lifts it.").small().weak());
            }
        }

        ui.add_space(8.0);
        section(ui, "Blocks");
        let Some(catalog) = doc.catalog.clone() else {
            let hint = if session.assets.is_some() { "This environment isn't in the game asset export." } else { "Choose the game asset folder to place blocks." };
            ui.label(egui::RichText::new(hint).small().weak());
            return;
        };
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("search"));
            if ui.add_enabled(self.brush.is_some(), egui::Button::new("Select")).on_hover_text("Stop placing (Esc)").clicked() {
                self.brush = None;
            }
        });
        let needle = self.search.to_lowercase();
        egui::ScrollArea::vertical().id_salt("mapedit-palette").auto_shrink(false).show(ui, |ui| {
            for name in catalog.names.iter().filter(|n| n.to_lowercase().contains(&needle)) {
                let chosen = self.brush.as_deref() == Some(name.as_str());
                if ui.selectable_label(chosen, name).clicked() {
                    self.brush = (!chosen).then(|| name.clone());
                    doc.selected = None;
                }
            }
        });
    }

    fn keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let Some(doc) = self.doc.as_mut() else { return };
        let (command, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |key| ui.input(|i| i.key_pressed(key));
        if command && pressed(Key::Z) {
            if shift {
                doc.redo();
            } else {
                doc.undo();
            }
            return;
        }
        if pressed(Key::Escape) {
            self.brush = None;
            doc.selected = None;
        }
        let top = doc.blocks().size[1] - 1;
        let lift = if pressed(Key::PageUp) { 1 } else if pressed(Key::PageDown) { -1 } else { 0 };
        let selected = doc.selected;
        if let (Some(i), true) = (selected, shift && lift != 0) {
            match doc.transform(i, (0, lift, 0), false) {
                Ok(()) => self.level = doc.map_block(i).y,
                Err(e) => self.message = Some(e),
            }
        } else if lift != 0 {
            self.level = (self.level + lift).clamp(0, top);
        }
        if pressed(Key::R) {
            match (selected, self.brush.is_some()) {
                (_, true) => self.brush_dir = (self.brush_dir + 1) % 4,
                (Some(i), false) => {
                    if let Err(e) = doc.transform(i, (0, 0, 0), true) {
                        self.message = Some(e);
                    }
                }
                _ => {}
            }
        }
        let Some(i) = selected else { return };
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            doc.delete(i);
            return;
        }
        // Screen up is toward -Z
        let step = [(Key::ArrowUp, (0, 0, -1)), (Key::ArrowDown, (0, 0, 1)), (Key::ArrowLeft, (-1, 0, 0)), (Key::ArrowRight, (1, 0, 0))];
        for (key, delta) in step {
            if pressed(key) {
                if let Err(e) = doc.transform(i, delta, false) {
                    self.message = Some(e);
                }
            }
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.doc.as_mut() else { return };
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let center = rect.center();
        let size = doc.blocks().size;
        if !self.fitted {
            let span = Vec2::new(size[0] as f32, size[2] as f32);
            self.view = View { center: [span.x / 2.0, span.y / 2.0], scale: ((rect.width() / span.x).min(rect.height() / span.y) * 0.92).max(1.0) };
            self.fitted = true;
        }
        if response.dragged() {
            let d = response.drag_delta() / self.view.scale;
            self.view.center[0] -= d.x;
            self.view.center[1] -= d.y;
        }
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.004).exp() * zoom;
            if factor != 1.0 {
                let anchor = response.hover_pos().unwrap_or(center);
                let before = (anchor - center) / self.view.scale;
                self.view.scale = (self.view.scale * factor).clamp(2.0, 120.0);
                let after = (anchor - center) / self.view.scale;
                self.view.center[0] += before.x - after.x;
                self.view.center[1] += before.y - after.y;
            }
        }
        let hover = response.hover_pos().map(|p| self.view.cell_at(p, center));

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(12, 16, 24));
        let bounds = Rect::from_two_pos(self.view.to_screen(0.0, 0.0, center), self.view.to_screen(size[0] as f32, size[2] as f32, center));
        painter.rect_filled(bounds, 0.0, INK);
        if self.view.scale >= 6.0 {
            let line = Stroke::new(1.0, INK_RAISED);
            for x in 0..=size[0] {
                painter.line_segment([self.view.to_screen(x as f32, 0.0, center), self.view.to_screen(x as f32, size[2] as f32, center)], line);
            }
            for z in 0..=size[2] {
                painter.line_segment([self.view.to_screen(0.0, z as f32, center), self.view.to_screen(size[0] as f32, z as f32, center)], line);
            }
        }

        // Blocks below this level first, faint, then this level's blocks with their facing
        let level = self.level;
        let blocks: Vec<(usize, Vec<Cell>, Kind)> = (0..doc.blocks().blocks.len())
            .filter(|&i| !doc.blocks().blocks[i].is_clip())
            .map(|i| (i, doc.cells(i), doc.kind(&doc.blocks().blocks[i].name)))
            .collect();
        if self.show_other_levels {
            let mut below: Vec<(i32, Rect, Kind)> = blocks
                .iter()
                .flat_map(|(_, cells, kind)| cells.iter().filter(|c| c.1 < level).map(|c| (c.1, self.view.cell_rect(c.0, c.2, center), *kind)))
                .collect();
            below.sort_by_key(|b| b.0);
            for (y, r, kind) in below {
                let fade = (40 - (level - y).min(6) * 5) as u8;
                painter.rect_filled(r.shrink(1.0), 1.0, kind_color(kind).gamma_multiply(fade as f32 / 100.0));
            }
        }
        for (i, cells, kind) in &blocks {
            let here: Vec<&Cell> = cells.iter().filter(|c| c.1 == level).collect();
            if here.is_empty() {
                continue;
            }
            for c in &here {
                let r = self.view.cell_rect(c.0, c.2, center);
                painter.rect_filled(r.shrink(1.0), 2.0, kind_color(*kind));
            }
            // An arrow toward the block's facing, in the middle of its cells on this level
            if self.view.scale >= 8.0 {
                let mid = here.iter().fold(Vec2::ZERO, |acc, c| acc + Vec2::new(c.0 as f32 + 0.5, c.2 as f32 + 0.5)) / here.len() as f32;
                let dir = doc.blocks().blocks[*i].dir as usize;
                arrow(&painter, self.view.to_screen(mid.x, mid.y, center), POINTING[dir], self.view.scale * 0.3, Color32::from_black_alpha(150));
            }
        }
        if let Some(i) = doc.selected {
            for c in doc.cells(i) {
                let stroke = if c.1 == level { Stroke::new(2.5, FINISH) } else { Stroke::new(1.0, FINISH.gamma_multiply(0.5)) };
                painter.rect_stroke(self.view.cell_rect(c.0, c.2, center), 2.0, stroke, egui::StrokeKind::Inside);
            }
        }

        let Some((hx, hz)) = hover else { return };
        let cell = (hx, level, hz);
        match self.brush.clone() {
            Some(name) => {
                let on_ground = doc.catalog.as_ref().is_some_and(|c| c.places_on_ground(&name, level));
                let cells = doc.cells_for(&name, cell, self.brush_dir, on_ground);
                let fits = doc.in_bounds(&cells) && !doc.overlaps(&cells, None);
                let tint = if fits { Color32::from_rgba_unmultiplied(120, 230, 140, 110) } else { Color32::from_rgba_unmultiplied(240, 80, 80, 110) };
                for c in &cells {
                    painter.rect_filled(self.view.cell_rect(c.0, c.2, center), 2.0, tint);
                }
                arrow(&painter, self.view.to_screen(hx as f32 + 0.5, hz as f32 + 0.5, center), POINTING[self.brush_dir as usize], self.view.scale * 0.3, TEXT);
                if response.clicked() {
                    if let Err(e) = doc.place(&name, cell, self.brush_dir) {
                        self.message = Some(e);
                    }
                }
                response.on_hover_text_at_pointer(format!("{name}  facing {}  (R turns)", DIRECTIONS[self.brush_dir as usize]));
            }
            None => {
                let under = doc.block_at(cell);
                painter.rect_stroke(self.view.cell_rect(hx, hz, center), 0.0, Stroke::new(1.0, TEXT.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                if response.clicked() {
                    doc.selected = under;
                }
                if let Some(i) = under {
                    response.on_hover_text_at_pointer(&doc.blocks().blocks[i].name);
                }
            }
        }
    }
}

fn arrow(painter: &egui::Painter, at: Pos2, (dx, dz): (f32, f32), size: f32, color: Color32) {
    let forward = Vec2::new(dx, dz) * size;
    let side = Vec2::new(-dz, dx) * size * 0.6;
    painter.add(Shape::convex_polygon(vec![at + forward, at - forward * 0.6 + side, at - forward * 0.6 - side], color, Stroke::NONE));
}

impl Module for MapEditModule {
    fn name(&self) -> &str {
        "Map Editor"
    }

    fn description(&self) -> &str {
        "Edit a map's blocks level by level and save it as a new map for the game"
    }

    fn save_settings(&self, s: &mut Settings) {
        s.insert("mapedit.author".into(), self.author.clone());
        s.insert("mapedit.show_other_levels".into(), self.show_other_levels.to_string());
    }

    fn load_settings(&mut self, s: &Settings) {
        self.author = setting(s, "mapedit.author").unwrap_or_default();
        self.show_other_levels = setting(s, "mapedit.show_other_levels").unwrap_or(self.show_other_levels);
    }

    fn ui(&mut self, ui: &mut egui::Ui, session: &mut Session, _wgpu: Option<&tmtools_core::RenderState>) {
        // With nothing open yet, start on the focused run's map
        if self.doc.is_none() && self.message.is_none() {
            if let Some(file) = session.focus.map(|r| &session.files[session.runs[r].file]) {
                if let Ok(doc) = open_bytes(&file.bytes, file.title()) {
                    self.open(doc);
                }
            }
        }
        self.ensure_catalog(session);
        self.toolbar(ui, session);
        if let Some(message) = &self.message {
            ui.label(egui::RichText::new(message).small().color(WEAK));
        }
        if self.doc.is_none() {
            ui.centered_and_justified(|ui| ui.label("Open a map, or pick a loaded replay's map, to edit its blocks."));
            return;
        }
        self.keys(ui);
        ui.horizontal_top(|ui| {
            let height = ui.available_height();
            ui.allocate_ui_with_layout(Vec2::new(SIDE_WIDTH, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(SIDE_WIDTH);
                self.side_panel(ui, session);
            });
            ui.separator();
            self.canvas(ui);
        });
    }
}
