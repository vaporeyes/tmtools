// ABOUTME: Opens replays from a gbx-tools-3d-node replay service: lists its library, downloads a replay's original file
// ABOUTME: into a local cache (so the session can reopen it), and shows a searchable window to pick from.

use eframe::egui;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use tmreplay::format_race_time;
use tmtools_core::Session;

pub const DEFAULT_URL: &str = "http://localhost:5080";

/// One replay in the service's library, as GET /replays lists it
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub file_name: String,
    pub label: Option<String>,
    pub map_name: Option<String>,
    pub player: Option<String>,
    pub environment: Option<String>,
    pub race_time_ms: Option<i64>,
}

impl Entry {
    fn map(&self) -> String {
        self.map_name.as_deref().map(|m| tmreplay::strip_formatting(m).trim().to_owned()).unwrap_or_else(|| self.file_name.clone())
    }

    fn player(&self) -> String {
        self.player.as_deref().map(|p| tmreplay::strip_formatting(p).trim().to_owned()).unwrap_or_default()
    }

    /// Whether every word of the search appears in the map, player, label, or file name
    fn matches(&self, search: &str) -> bool {
        let haystack = format!("{} {} {} {}", self.map(), self.player(), self.label.as_deref().unwrap_or(""), self.file_name).to_lowercase();
        search.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
    }
}

/// The base URL without its user and password, and an Authorization header for them (private sites use basic auth)
pub fn split_credentials(url: &str) -> (String, Option<String>) {
    let url = url.trim().trim_end_matches('/');
    let Some((scheme, rest)) = url.split_once("://") else { return (url.to_owned(), None) };
    let host_end = rest.find('/').unwrap_or(rest.len());
    match rest[..host_end].rsplit_once('@') {
        Some((userinfo, host)) => (format!("{scheme}://{host}{}", &rest[host_end..]), Some(format!("Basic {}", base64(userinfo.as_bytes())))),
        None => (url.to_owned(), None),
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Where downloaded replays are kept, by library id
pub fn cache_path(id: &str) -> Option<PathBuf> {
    // Library ids are 16 hex characters; anything else could name a path outside the cache
    (id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| dirs::cache_dir().map(|d| d.join("tmtools").join("library").join(format!("{id}.Replay.Gbx"))))?
}

fn get(url: &str, auth: Option<&str>) -> Result<ureq::Response, String> {
    let mut request = ureq::get(url).timeout(std::time::Duration::from_secs(30));
    if let Some(auth) = auth {
        request = request.set("Authorization", auth);
    }
    request.call().map_err(|e| match e {
        ureq::Error::Status(401, _) => "The service asks for a user and password; put them in the URL as https://user:password@host".to_owned(),
        ureq::Error::Status(code, _) => format!("The service answered {code}"),
        other => format!("Couldn't reach the service: {other}"),
    })
}

fn fetch_list(base: &str, auth: Option<&str>) -> Result<Vec<Entry>, String> {
    let text = get(&format!("{base}/replays"), auth)?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("The service's list isn't readable: {e}"))
}

fn fetch_file(base: &str, auth: Option<&str>, id: &str) -> Result<PathBuf, String> {
    let path = cache_path(id).ok_or("No cache folder for downloads on this system")?;
    if !path.exists() {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut get(&format!("{base}/replays/{id}/file"), auth)?.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(path.parent().expect("cache files have a folder")).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

enum Reply {
    List(Result<Vec<Entry>, String>),
    File(Result<PathBuf, String>),
}

pub struct LibraryWindow {
    pub open: bool,
    pub url: String,
    entries: Vec<Entry>,
    search: String,
    status: Option<String>,
    pending: Option<Receiver<Reply>>,
}

impl Default for LibraryWindow {
    fn default() -> Self {
        Self { open: false, url: DEFAULT_URL.into(), entries: Vec::new(), search: String::new(), status: None, pending: None }
    }
}

impl LibraryWindow {
    fn start(&mut self, work: impl FnOnce(String, Option<String>) -> Reply + Send + 'static) {
        let (base, auth) = split_credentials(&self.url);
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let _ = sender.send(work(base, auth));
        });
        self.pending = Some(receiver);
    }

    pub fn refresh(&mut self) {
        self.status = Some("Loading the library...".into());
        self.start(|base, auth| Reply::List(fetch_list(&base, auth.as_deref())));
    }

    fn download(&mut self, id: String) {
        self.status = Some("Downloading...".into());
        self.start(move |base, auth| Reply::File(fetch_file(&base, auth.as_deref(), &id)));
    }

    pub fn ui(&mut self, ctx: &egui::Context, session: &mut Session) {
        if let Some(reply) = self.pending.as_ref().and_then(|r| r.try_recv().ok()) {
            self.pending = None;
            match reply {
                Reply::List(Ok(entries)) => {
                    self.status = Some(format!("{} replays", entries.len()));
                    self.entries = entries;
                }
                Reply::File(Ok(path)) => {
                    session.load_path(&path);
                    self.status = session.status.clone();
                }
                Reply::List(Err(e)) | Reply::File(Err(e)) => self.status = Some(e),
            }
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        let mut open = self.open;
        egui::Window::new("Replay library").open(&mut open).default_size([640.0, 480.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Service");
                ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(260.0))
                    .on_hover_text("The gbx-tools-3d-node replay service, such as http://localhost:5080; for a private site, https://user:password@host");
                if ui.add_enabled(self.pending.is_none(), egui::Button::new("Refresh")).clicked() {
                    self.refresh();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("map or player").desired_width(260.0));
            });
            if let Some(status) = &self.status {
                ui.label(egui::RichText::new(status).small().weak());
            }
            ui.separator();

            let mut chosen = None;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                egui::Grid::new("library-entries").striped(true).num_columns(5).show(ui, |ui| {
                    for header in ["Map", "Player", "Time", "Environment", ""] {
                        ui.strong(header);
                    }
                    ui.end_row();
                    for e in self.entries.iter().filter(|e| e.matches(&self.search)) {
                        ui.label(e.map()).on_hover_text(e.label.as_deref().unwrap_or(&e.file_name));
                        ui.label(e.player());
                        ui.label(e.race_time_ms.map_or("-".into(), format_race_time));
                        ui.label(e.environment.as_deref().unwrap_or(""));
                        if ui.add_enabled(self.pending.is_none(), egui::Button::new("Open")).clicked() {
                            chosen = Some(e.id.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(id) = chosen {
                self.download(id);
            }
        });
        self.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_credentials_out_of_the_url() {
        assert_eq!(split_credentials("http://localhost:5080/"), ("http://localhost:5080".to_owned(), None));
        let (base, auth) = split_credentials("https://jsh:hunter2@replays.example.com");
        assert_eq!(base, "https://replays.example.com");
        assert_eq!(auth.as_deref(), Some("Basic anNoOmh1bnRlcjI="));
    }

    #[test]
    fn encodes_base64_with_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
    }

    #[test]
    fn cache_paths_only_for_library_ids() {
        assert!(cache_path("3279b1562115afbf").is_some_and(|p| p.ends_with("tmtools/library/3279b1562115afbf.Replay.Gbx")));
        assert!(cache_path("../../etc/passwd").is_none());
        assert!(cache_path("3279b1562115afb").is_none());
    }

    // Against a running service: set TMTOOLS_SERVICE_URL (such as http://localhost:5080); skipped without it
    #[test]
    fn lists_and_downloads_from_a_running_service() {
        let Ok(url) = std::env::var("TMTOOLS_SERVICE_URL") else {
            eprintln!("TMTOOLS_SERVICE_URL is not set; skipping the live library test");
            return;
        };
        let (base, auth) = split_credentials(&url);
        let entries = fetch_list(&base, auth.as_deref()).unwrap();
        let first = entries.first().expect("the library has replays");
        let path = fetch_file(&base, auth.as_deref(), &first.id).unwrap();
        let replay = tmreplay::parse_replay(&std::fs::read(&path).unwrap()).unwrap();
        assert!(!replay.ghosts.is_empty());
    }

    #[test]
    fn reads_the_service_list_and_searches_it() {
        let json = r#"[{"id":"3279b1562115afbf","fileName":"a.Replay.Gbx","label":null,"addedAt":"x","mapName":"$FFF$i ShortLOL $F10#$FFF8","mapUid":"u","environment":"Stadium","player":"Alexbros","login":"l","raceTimeMs":12850,"tmx":null}]"#;
        let entries: Vec<Entry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries[0].map(), "ShortLOL #8");
        assert!(entries[0].matches("shortlol alex"));
        assert!(!entries[0].matches("interlock"));
    }
}
