// ABOUTME: Saving and restoring a session between launches: which replays were open, the focused and hidden runs,
// ABOUTME: placed sector splits, the clock, the active tab, and module settings, as JSON the shell stores.

use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use tmreplay::Vec3;

/// Module settings as text, keyed "module.setting"
pub type Settings = BTreeMap<String, String>;

/// A setting parsed back from text; None when absent or unreadable
pub fn setting<T: FromStr>(settings: &Settings, key: &str) -> Option<T> {
    settings.get(key).and_then(|v| v.parse().ok())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedSession {
    /// Files opened from disk, in order; files without a path aren't saved
    pub files: Vec<PathBuf>,
    /// The focused run as (index into files, ghost)
    pub focus: Option<(usize, usize)>,
    /// Runs switched off, as (index into files, ghost)
    pub hidden: Vec<(usize, usize)>,
    pub splits: BTreeMap<String, Vec<[f32; 3]>>,
    pub time_ms: f64,
    pub speed: f64,
    pub active_tab: usize,
    pub settings: Settings,
}

impl SavedSession {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

impl Session {
    /// What to save of this session; module settings and the tab are filled in by the shell
    pub fn save(&self) -> SavedSession {
        // Index among saved files, for files that have a path
        let mut saved_index = vec![None; self.files.len()];
        let mut files = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            if let Some(path) = &f.path {
                saved_index[i] = Some(files.len());
                files.push(path.clone());
            }
        }
        let key = |run: usize| saved_index[self.runs[run].file].map(|f| (f, self.runs[run].ghost));
        SavedSession {
            files,
            focus: self.focus.and_then(key),
            hidden: (0..self.runs.len()).filter(|&r| !self.runs[r].visible).filter_map(key).collect(),
            splits: self.splits.iter().map(|(k, v)| (k.clone(), v.iter().map(|p| [p.x, p.y, p.z]).collect())).collect(),
            time_ms: self.playback.time_ms,
            speed: self.playback.speed,
            ..Default::default()
        }
    }

    /// Restores splits and the clock, and, when `files` is true, reopens the saved files that still exist with their
    /// focus and visibility
    pub fn restore(&mut self, saved: &SavedSession, files: bool) {
        for (key, points) in &saved.splits {
            self.splits.insert(key.clone(), points.iter().map(|p| Vec3 { x: p[0], y: p[1], z: p[2] }).collect());
        }
        if crate::SPEEDS.contains(&saved.speed) {
            self.playback.speed = saved.speed;
        }
        if files {
            let mut opened: Vec<Option<usize>> = Vec::new();
            for path in &saved.files {
                let before = self.files.len();
                if path.exists() {
                    self.load_path(path);
                }
                opened.push((self.files.len() > before).then_some(before));
            }
            let run_of = |(file, ghost): (usize, usize)| {
                let file = opened.get(file).copied().flatten()?;
                self.runs.iter().position(|r| r.file == file && r.ghost == ghost)
            };
            let hidden: Vec<usize> = saved.hidden.iter().filter_map(|&k| run_of(k)).collect();
            if let Some(focus) = saved.focus.and_then(run_of) {
                self.focus = Some(focus);
            }
            for r in hidden {
                self.runs[r].visible = false;
            }
            self.status = (!self.files.is_empty()).then(|| format!("Reopened {} file{} from last time", self.files.len(), if self.files.len() == 1 { "" } else { "s" }));
        }
        self.seek(saved.time_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{LoadedFile, Run};
    use egui::Color32;
    use std::sync::Arc;
    use tmreplay::{Ghost, Replay};

    fn session_with(paths: &[Option<&str>]) -> Session {
        let mut s = Session::default();
        for (file, path) in paths.iter().enumerate() {
            let replay = Replay { ghosts: vec![Ghost { race_time_ms: Some(5000), ..Default::default() }], ..Default::default() };
            s.files.push(LoadedFile { path: path.map(PathBuf::from), name: format!("{file}"), bytes: Arc::new(Vec::new()), replay: Ok(replay) });
            s.runs.push(Run { file, ghost: 0, label: String::new(), color: Color32::WHITE, visible: true, track: None });
        }
        s
    }

    #[test]
    fn saves_files_with_paths_and_runs_by_file_and_ghost() {
        let mut s = session_with(&[Some("/a.Replay.Gbx"), None, Some("/c.Replay.Gbx")]);
        s.focus = Some(2);
        s.runs[0].visible = false;
        s.splits.insert("map".into(), vec![Vec3 { x: 1.0, y: 2.0, z: 3.0 }]);
        s.playback.time_ms = 1234.0;
        let saved = s.save();
        assert_eq!(saved.files, [PathBuf::from("/a.Replay.Gbx"), PathBuf::from("/c.Replay.Gbx")]);
        // The file without a path is left out, so the third file is saved second
        assert_eq!(saved.focus, Some((1, 0)));
        assert_eq!(saved.hidden, [(0, 0)]);
        assert_eq!(saved.splits["map"], [[1.0, 2.0, 3.0]]);

        let back = SavedSession::from_json(&saved.to_json()).unwrap();
        assert_eq!(back, saved);
        assert!(SavedSession::from_json("not json").is_none());
    }

    #[test]
    fn restores_splits_and_the_clock_without_reopening_files() {
        let mut saved = SavedSession { time_ms: 2000.0, speed: 0.25, ..Default::default() };
        saved.splits.insert("map".into(), vec![[4.0, 5.0, 6.0]]);
        saved.files.push(PathBuf::from("/does/not/exist.Replay.Gbx"));
        let mut s = session_with(&[Some("/a.Replay.Gbx")]);
        s.restore(&saved, true);
        assert_eq!(s.files.len(), 1, "missing files are skipped");
        assert_eq!(s.splits["map"][0], Vec3 { x: 4.0, y: 5.0, z: 6.0 });
        assert_eq!((s.playback.time_ms, s.playback.speed), (2000.0, 0.25));
    }

    #[test]
    fn reads_settings_back() {
        let mut settings = Settings::new();
        settings.insert("view.ghosts".into(), "true".into());
        settings.insert("view.opacity".into(), "0.4".into());
        assert_eq!(setting::<bool>(&settings, "view.ghosts"), Some(true));
        assert_eq!(setting::<f32>(&settings, "view.opacity"), Some(0.4));
        assert_eq!(setting::<f32>(&settings, "view.missing"), None);
    }
}
