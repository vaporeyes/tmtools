// ABOUTME: The piano roll's horizontal view: which race times are on screen and how many milliseconds a pixel spans,
// ABOUTME: with zoom about a point, following the playhead, and grid steps that stay readable at any zoom.

/// Milliseconds a pixel may span, from a tick every 12 pixels out to whole long runs
const MIN_MS_PER_PX: f64 = 10.0 / 12.0;
const MAX_MS_PER_PX: f64 = 500.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    /// Race time at the left edge
    pub start_ms: f64,
    pub ms_per_px: f64,
}

impl Default for TimeView {
    fn default() -> Self {
        // About two seconds across a typical window, with ticks visible
        Self { start_ms: -100.0, ms_per_px: 2.0 }
    }
}

impl TimeView {
    pub fn x_of(&self, time_ms: f64) -> f64 {
        (time_ms - self.start_ms) / self.ms_per_px
    }

    pub fn time_at(&self, x: f64) -> f64 {
        self.start_ms + x * self.ms_per_px
    }

    /// Zooms by factor (above 1 zooms out) keeping anchor_x over the same time
    pub fn zoom_about(&mut self, factor: f64, anchor_x: f64) {
        let anchor_ms = self.time_at(anchor_x);
        self.ms_per_px = (self.ms_per_px * factor).clamp(MIN_MS_PER_PX, MAX_MS_PER_PX);
        self.start_ms = anchor_ms - anchor_x * self.ms_per_px;
    }

    pub fn pan_px(&mut self, dx: f64) {
        self.start_ms -= dx * self.ms_per_px;
    }

    /// Scrolls so time_ms sits inside the middle of the view, leaving a fifth of the width on either side
    pub fn follow(&mut self, time_ms: f64, width_px: f64) {
        let margin = width_px * 0.2;
        let x = self.x_of(time_ms);
        if x < margin {
            self.start_ms = time_ms - margin * self.ms_per_px;
        } else if x > width_px - margin {
            self.start_ms = time_ms - (width_px - margin) * self.ms_per_px;
        }
    }

    /// Shows the whole span from 0 to duration_ms across width_px
    pub fn fit(&mut self, duration_ms: f64, width_px: f64) {
        self.ms_per_px = (duration_ms.max(100.0) * 1.02 / width_px.max(1.0)).clamp(MIN_MS_PER_PX, MAX_MS_PER_PX);
        self.start_ms = -duration_ms * 0.01;
    }

    /// Pixels between ticks
    pub fn tick_px(&self) -> f64 {
        10.0 / self.ms_per_px
    }

    /// The smallest 1-2-5 step of whole ticks whose labels are at least min_px apart
    pub fn label_step_ms(&self, min_px: f64) -> i64 {
        let mut step = 10;
        let mut k = 0;
        while (step as f64) / self.ms_per_px < min_px {
            step = [20, 50, 100][k % 3] * 10i64.pow(k as u32 / 3);
            k += 1;
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_time_and_pixels_both_ways() {
        let view = TimeView { start_ms: 1000.0, ms_per_px: 2.0 };
        assert_eq!(view.x_of(1200.0), 100.0);
        assert_eq!(view.time_at(100.0), 1200.0);
    }

    #[test]
    fn zooms_about_the_anchor() {
        let mut view = TimeView { start_ms: 0.0, ms_per_px: 2.0 };
        view.zoom_about(0.5, 300.0);
        assert_eq!(view.ms_per_px, 1.0);
        assert_eq!(view.time_at(300.0), 600.0);
        view.zoom_about(1e-6, 0.0);
        assert_eq!(view.ms_per_px, MIN_MS_PER_PX);
    }

    #[test]
    fn follows_the_playhead_only_near_the_edges() {
        let mut view = TimeView { start_ms: 0.0, ms_per_px: 1.0 };
        view.follow(500.0, 1000.0);
        assert_eq!(view.start_ms, 0.0);
        view.follow(950.0, 1000.0);
        assert_eq!(view.x_of(950.0), 800.0);
        view.follow(10.0, 1000.0);
        assert_eq!(view.x_of(10.0), 200.0);
    }

    #[test]
    fn label_steps_grow_in_1_2_5_steps() {
        assert_eq!(TimeView { start_ms: 0.0, ms_per_px: 0.1 }.label_step_ms(60.0), 10);
        assert_eq!(TimeView { start_ms: 0.0, ms_per_px: 1.0 }.label_step_ms(60.0), 100);
        assert_eq!(TimeView { start_ms: 0.0, ms_per_px: 2.0 }.label_step_ms(60.0), 200);
        assert_eq!(TimeView { start_ms: 0.0, ms_per_px: 20.0 }.label_step_ms(60.0), 2000);
    }
}
