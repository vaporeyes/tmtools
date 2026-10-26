// ABOUTME: The tmtools look: a dark theme in the replay viewer family's colors (ink panels, the finish-line yellow
// ABOUTME: accent) and shared widgets every module uses, so tabs look alike: toolbars, section headings, icon buttons.

use egui::{Color32, CornerRadius, Margin, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2};

/// The deepest background, behind the 3D view and plots
pub const NIGHT: Color32 = Color32::from_rgb(11, 17, 30);
/// Panels
pub const INK: Color32 = Color32::from_rgb(19, 33, 58);
/// Toolbars and raised strips
pub const INK_RAISED: Color32 = Color32::from_rgb(28, 45, 76);
/// Controls at rest, and borders
pub const INK_SOFT: Color32 = Color32::from_rgb(42, 58, 85);
pub const HOVER: Color32 = Color32::from_rgb(55, 76, 110);
/// The accent: the finish-line yellow of the replay viewer
pub const FINISH: Color32 = Color32::from_rgb(255, 210, 63);
pub const TEXT: Color32 = Color32::from_rgb(230, 235, 242);
pub const WEAK: Color32 = Color32::from_rgb(138, 148, 158);

/// Applies the theme to every style (dark and light alike, so the app always looks the same)
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = INK;
        v.window_fill = INK;
        v.extreme_bg_color = NIGHT;
        v.faint_bg_color = Color32::from_rgb(24, 39, 66);
        v.code_bg_color = NIGHT;
        v.hyperlink_color = Color32::from_rgb(120, 180, 255);
        v.override_text_color = None;
        v.weak_text_color = Some(WEAK);
        v.window_stroke = Stroke::new(1.0, INK_SOFT);
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(6);
        // Selected tabs and items are yellow with dark text, like the viewer's highlights
        v.selection.bg_fill = FINISH;
        v.selection.stroke = Stroke::new(1.0, INK);
        v.slider_trailing_fill = true;

        let radius = CornerRadius::same(5);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = INK;
        w.noninteractive.weak_bg_fill = INK;
        w.noninteractive.bg_stroke = Stroke::new(1.0, INK_SOFT);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        for (state, fill) in [(&mut w.inactive, INK_SOFT), (&mut w.hovered, HOVER), (&mut w.active, HOVER), (&mut w.open, INK_SOFT)] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.corner_radius = radius;
            state.fg_stroke = Stroke::new(1.0, TEXT);
        }
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.bg_stroke = Stroke::new(1.0, FINISH.gamma_multiply(0.6));
        w.active.bg_stroke = Stroke::new(1.0, FINISH);
        w.noninteractive.corner_radius = radius;

        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(8.0, 4.0);
        s.interact_size.y = 24.0;
        s.window_margin = Margin::same(10);
    });
}

/// A raised strip across the top of a module for its controls, wrapping onto more lines when narrow
pub fn toolbar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(INK_RAISED)
        .inner_margin(Margin::symmetric(10, 6))
        .corner_radius(CornerRadius::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(add).inner
        })
        .inner
}

/// A small uppercase label over a group of controls or a table
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title.to_uppercase()).small().strong().color(WEAK));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    StepBack,
    StepForward,
    ToStart,
    Close,
}

/// A square button drawing its icon with shapes, so it needs no symbol font; `accent` makes it the yellow main action
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, accent: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let response = response.on_hover_text(tooltip);
    let painter = ui.painter();
    let (fill, ink) = match (accent, response.hovered()) {
        (true, false) => (FINISH, INK),
        (true, true) => (Color32::from_rgb(255, 224, 120), INK),
        (false, false) => (INK_SOFT, TEXT),
        (false, true) => (HOVER, TEXT),
    };
    if accent {
        painter.circle_filled(rect.center(), size / 2.0, fill);
    } else {
        painter.rect_filled(rect, CornerRadius::same(5), fill);
    }
    for shape in icon_shapes(icon, rect.shrink(size * 0.3), ink) {
        painter.add(shape);
    }
    response
}

/// The icon drawn into r
pub fn icon_shapes(icon: Icon, r: Rect, ink: Color32) -> Vec<Shape> {
    let tri = |a: Pos2, b: Pos2, c: Pos2| Shape::convex_polygon(vec![a, b, c], ink, Stroke::NONE);
    let bar = |x0: f32, x1: f32| Shape::rect_filled(Rect::from_min_max(Pos2::new(x0, r.top()), Pos2::new(x1, r.bottom())), 0.0, ink);
    let (l, rt, t, b, cy) = (r.left(), r.right(), r.top(), r.bottom(), r.center().y);
    let w = r.width();
    match icon {
        Icon::Play => vec![tri(Pos2::new(l + w * 0.12, t), Pos2::new(rt, cy), Pos2::new(l + w * 0.12, b))],
        Icon::Pause => vec![bar(l + w * 0.1, l + w * 0.38), bar(l + w * 0.62, l + w * 0.9)],
        Icon::StepBack => vec![bar(l, l + w * 0.18), tri(Pos2::new(rt, t), Pos2::new(l + w * 0.22, cy), Pos2::new(rt, b))],
        Icon::StepForward => vec![tri(Pos2::new(l, t), Pos2::new(rt - w * 0.22, cy), Pos2::new(l, b)), bar(rt - w * 0.18, rt)],
        Icon::ToStart => vec![
            bar(l, l + w * 0.15),
            tri(Pos2::new(l + w * 0.6, t), Pos2::new(l + w * 0.18, cy), Pos2::new(l + w * 0.6, b)),
            tri(Pos2::new(rt, t), Pos2::new(l + w * 0.58, cy), Pos2::new(rt, b)),
        ],
        Icon::Close => {
            let s = Stroke::new(w * 0.16, ink);
            vec![Shape::line_segment([r.left_top(), r.right_bottom()], s), Shape::line_segment([r.right_top(), r.left_bottom()], s)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_stay_inside_their_box() {
        let r = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(20.0));
        for icon in [Icon::Play, Icon::Pause, Icon::StepBack, Icon::StepForward, Icon::ToStart, Icon::Close] {
            let shapes = icon_shapes(icon, r, Color32::WHITE);
            assert!(!shapes.is_empty());
            for s in shapes {
                let bounds = s.visual_bounding_rect();
                assert!(r.expand(2.0).contains_rect(bounds), "{icon:?} spills out: {bounds:?}");
            }
        }
    }
}
