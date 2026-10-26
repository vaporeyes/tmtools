// ABOUTME: Adds a system Unicode font as a fallback, so player names in Japanese, Korean, and other scripts render.
// ABOUTME: egui's bundled fonts cover Latin text only; the first font found on this machine is used.

use eframe::egui;
use std::sync::Arc;

const CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "C:\\Windows\\Fonts\\arialuni.ttf",
    "C:\\Windows\\Fonts\\msgothic.ttc",
];

pub fn add_unicode_fallback(ctx: &egui::Context) {
    let Some(bytes) = CANDIDATES.iter().find_map(|path| std::fs::read(path).ok()) else { return };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("unicode-fallback".into(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("unicode-fallback".into());
    }
    ctx.set_fonts(fonts);
}
