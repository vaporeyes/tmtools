// ABOUTME: The shared session: loaded Gbx files, the runs (ghosts) in them with colors and visibility, the focused
// ABOUTME: run, the playback clock, and the game asset export. Pure state and logic, so it is tested without a window.

use egui::Color32;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tmassets::AssetStore;
use tmreplay::track::{InputTrack, TICK_MS};
use tmreplay::{parse_replay, Ghost, Replay, Vec3};

/// Run colors, picked in order and reused after the last
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(255, 196, 0),
    Color32::from_rgb(0, 170, 255),
    Color32::from_rgb(255, 90, 120),
    Color32::from_rgb(90, 220, 120),
    Color32::from_rgb(190, 120, 255),
    Color32::from_rgb(255, 140, 50),
    Color32::from_rgb(80, 220, 220),
    Color32::from_rgb(230, 230, 230),
];

/// Playback speeds offered in the shell
pub const SPEEDS: [f64; 7] = [0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0];

pub struct LoadedFile {
    pub path: Option<PathBuf>,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    /// The parsed replay, or why the file isn't one (it may still be any Gbx, open to the inspector)
    pub replay: Result<Replay, String>,
}

impl LoadedFile {
    /// What to call the file: its map's name without formatting codes, else the file name
    pub fn title(&self) -> String {
        self.replay
            .as_ref()
            .ok()
            .and_then(|r| r.map.name.as_deref())
            .map(tmreplay::strip_formatting)
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.name.clone())
    }
}

pub struct Run {
    pub file: usize,
    pub ghost: usize,
    pub label: String,
    pub color: Color32,
    pub visible: bool,
    /// The run's inputs, exact to the tick; None when it has none
    pub track: Option<InputTrack>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    /// Race time in milliseconds
    pub time_ms: f64,
    pub playing: bool,
    pub speed: f64,
}

pub struct Session {
    pub files: Vec<LoadedFile>,
    pub runs: Vec<Run>,
    pub focus: Option<usize>,
    pub playback: Playback,
    /// The last load message, shown in the shell
    pub status: Option<String>,
    /// Meshes and textures exported from a TMF install, for drawing maps and cars as the game does
    pub assets: Option<Arc<AssetStore>>,
    /// Sector split points placed on each map, by map_key
    pub splits: HashMap<String, Vec<Vec3>>,
    next_color: usize,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            runs: Vec::new(),
            focus: None,
            playback: Playback { time_ms: 0.0, playing: false, speed: 1.0 },
            status: None,
            assets: None,
            splits: HashMap::new(),
            next_color: 0,
        }
    }
}

impl Session {
    pub fn load_path(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                self.load_bytes(name, bytes, Some(path.to_path_buf()));
            }
            Err(e) => self.status = Some(format!("Couldn't read {}: {e}", path.display())),
        }
    }

    /// Adds a file; replays add their ghosts as runs, and the first run loaded becomes the focus
    pub fn load_bytes(&mut self, name: String, bytes: Vec<u8>, path: Option<PathBuf>) {
        let replay = parse_replay(&bytes).map_err(|e| e.to_string());
        self.status = Some(match &replay {
            Ok(r) => {
                let map = r.map.name.as_deref().map(tmreplay::strip_formatting).unwrap_or_else(|| name.clone());
                format!("Loaded {}: {} ghost{}", map.trim(), r.ghosts.len(), if r.ghosts.len() == 1 { "" } else { "s" })
            }
            Err(e) => format!("{name} isn't a replay ({e}); it can still be inspected"),
        });
        let file = self.files.len();
        if let Ok(replay) = &replay {
            for (ghost, g) in replay.ghosts.iter().enumerate() {
                let color = PALETTE[self.next_color % PALETTE.len()];
                self.next_color += 1;
                let track = InputTrack::new(&g.inputs, g.duration_ms());
                self.runs.push(Run { file, ghost, label: run_label(g), color, visible: true, track });
            }
        }
        self.files.push(LoadedFile { path, name, bytes: Arc::new(bytes), replay });
        if self.focus.is_none() && !self.runs.is_empty() {
            self.focus = Some(0);
        }
    }

    /// Opens an asset export folder (the one holding api/, or api/ itself) for every module to draw from
    pub fn open_assets(&mut self, dir: &Path) -> Result<(), String> {
        let store = AssetStore::open(dir).map_err(|e| e.to_string())?;
        let environments = store.collections().map_err(|e| e.to_string())?.len();
        self.status = Some(format!("Game assets: {environments} environments from {}", dir.display()));
        self.assets = Some(Arc::new(store));
        Ok(())
    }

    /// Removes a file and its runs; the focus moves to the first remaining run when its run goes
    pub fn remove_file(&mut self, file: usize) {
        if file >= self.files.len() {
            return;
        }
        let focused = self.focus.map(|i| (self.runs[i].file, self.runs[i].ghost));
        self.files.remove(file);
        self.runs.retain(|r| r.file != file);
        for run in &mut self.runs {
            if run.file > file {
                run.file -= 1;
            }
        }
        self.focus = focused
            .and_then(|(f, g)| if f == file { None } else { Some((if f > file { f - 1 } else { f }, g)) })
            .and_then(|(f, g)| self.runs.iter().position(|r| r.file == f && r.ghost == g))
            .or((!self.runs.is_empty()).then_some(0));
        self.playback.time_ms = self.playback.time_ms.min(self.duration_ms());
    }

    pub fn replay(&self, run: usize) -> &Replay {
        self.files[self.runs[run].file].replay.as_ref().expect("runs come from parsed replays")
    }

    pub fn ghost(&self, run: usize) -> &Ghost {
        &self.replay(run).ghosts[self.runs[run].ghost]
    }

    pub fn visible_runs(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.runs.len()).filter(|&i| self.runs[i].visible)
    }

    /// The longest visible run, in milliseconds, so the clock can reach every finish
    pub fn duration_ms(&self) -> f64 {
        self.visible_runs().map(|i| self.ghost(i).duration_ms()).max().unwrap_or(0) as f64
    }

    /// Moves the clock by real seconds; stops at the end
    pub fn advance(&mut self, dt_seconds: f64) {
        if !self.playback.playing {
            return;
        }
        let end = self.duration_ms();
        self.playback.time_ms = (self.playback.time_ms + dt_seconds * 1000.0 * self.playback.speed).min(end);
        if self.playback.time_ms >= end {
            self.playback.playing = false;
        }
    }

    pub fn toggle_play(&mut self) {
        // Playing from the end starts the run over
        if !self.playback.playing && self.playback.time_ms >= self.duration_ms() {
            self.playback.time_ms = 0.0;
        }
        self.playback.playing = !self.playback.playing;
    }

    pub fn seek(&mut self, time_ms: f64) {
        self.playback.time_ms = time_ms.clamp(0.0, self.duration_ms());
    }

    /// Steps whole ticks from the current one, so repeated steps never drift
    pub fn step_ticks(&mut self, ticks: i32) {
        let tick = tmreplay::track::tick_of(self.playback.time_ms);
        self.playback.playing = false;
        self.seek((tick + ticks * TICK_MS) as f64);
    }

    /// Identifies a run's map: its UID, else the file name; runs on the same map share sector splits
    pub fn map_key(&self, run: usize) -> String {
        self.replay(run).map.uid.clone().unwrap_or_else(|| self.files[self.runs[run].file].name.clone())
    }

    /// Visible runs on the same map as the given run, that run first
    pub fn runs_on_map_of(&self, run: usize) -> Vec<usize> {
        let key = self.map_key(run);
        std::iter::once(run).chain(self.visible_runs().filter(|&r| r != run && self.map_key(r) == key)).collect()
    }

    /// Split points placed on the run's map
    pub fn splits_for(&self, run: usize) -> &[Vec3] {
        self.splits.get(&self.map_key(run)).map_or(&[], Vec::as_slice)
    }

    /// The current time as the tick it falls in
    pub fn tick_ms(&self) -> i32 {
        tmreplay::track::tick_of(self.playback.time_ms)
    }
}

fn run_label(ghost: &Ghost) -> String {
    match ghost.race_time_ms {
        Some(t) => format!("{} ({})", ghost.display_name(), tmreplay::format_race_time(t as i64)),
        None => ghost.display_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_runs(durations: &[i32]) -> Session {
        let mut s = Session::default();
        for (file, &d) in durations.iter().enumerate() {
            let ghost = Ghost { race_time_ms: Some(d), ..Default::default() };
            let replay = Replay { ghosts: vec![ghost], ..Default::default() };
            s.files.push(LoadedFile { path: None, name: format!("{file}"), bytes: Arc::new(Vec::new()), replay: Ok(replay) });
            s.runs.push(Run { file, ghost: 0, label: String::new(), color: Color32::WHITE, visible: true, track: None });
        }
        s.focus = Some(0);
        s
    }

    #[test]
    fn the_clock_runs_to_the_longest_visible_run_and_stops() {
        let mut s = session_with_runs(&[1000, 3000]);
        assert_eq!(s.duration_ms(), 3000.0);
        s.runs[1].visible = false;
        assert_eq!(s.duration_ms(), 1000.0);
        s.toggle_play();
        s.advance(0.5);
        assert_eq!(s.playback.time_ms, 500.0);
        s.advance(1.0);
        assert_eq!((s.playback.time_ms, s.playback.playing), (1000.0, false));
        // Playing again from the end starts over
        s.toggle_play();
        assert_eq!(s.playback.time_ms, 0.0);
    }

    #[test]
    fn steps_whole_ticks_and_pauses() {
        let mut s = session_with_runs(&[1000]);
        s.playback.playing = true;
        s.playback.time_ms = 123.4;
        s.step_ticks(1);
        assert_eq!((s.playback.time_ms, s.playback.playing), (130.0, false));
        s.step_ticks(-3);
        assert_eq!(s.playback.time_ms, 100.0);
        s.step_ticks(-20);
        assert_eq!(s.playback.time_ms, 0.0);
    }

    #[test]
    fn removing_a_file_keeps_the_focus_on_the_same_run() {
        let mut s = session_with_runs(&[1000, 2000, 3000]);
        s.focus = Some(2);
        s.remove_file(0);
        assert_eq!(s.runs.len(), 2);
        assert_eq!((s.runs[0].file, s.runs[1].file), (0, 1));
        assert_eq!(s.focus, Some(1));
        s.remove_file(1);
        assert_eq!(s.focus, Some(0));
    }

    #[test]
    fn files_are_titled_by_their_map_name() {
        let mut s = session_with_runs(&[1000]);
        assert_eq!(s.files[0].title(), "0");
        if let Ok(r) = &mut s.files[0].replay {
            r.map.name = Some("$o$n$s$fffInterlock #USM".into());
        }
        assert_eq!(s.files[0].title(), "Interlock #USM");
    }

    #[test]
    fn runs_on_the_same_map_share_splits() {
        let mut s = session_with_runs(&[1000, 2000, 3000]);
        for (i, uid) in ["a", "a", "b"].into_iter().enumerate() {
            if let Ok(r) = &mut s.files[i].replay {
                r.map.uid = Some(uid.into());
            }
        }
        assert_eq!(s.runs_on_map_of(1), [1, 0]);
        s.splits.entry(s.map_key(0)).or_default().push(Vec3 { x: 1.0, y: 2.0, z: 3.0 });
        assert_eq!(s.splits_for(1).len(), 1);
        assert!(s.splits_for(2).is_empty());
    }

    #[test]
    fn a_folder_that_isnt_an_export_leaves_assets_unset() {
        let mut s = Session::default();
        assert!(s.open_assets(Path::new("/nonexistent")).is_err());
        assert!(s.assets.is_none());
    }

    #[test]
    fn non_replay_files_are_kept_for_inspection() {
        let mut s = Session::default();
        s.load_bytes("junk.Gbx".into(), b"not a gbx".to_vec(), None);
        assert_eq!(s.files.len(), 1);
        assert!(s.files[0].replay.is_err());
        assert!(s.runs.is_empty() && s.focus.is_none());
    }
}
