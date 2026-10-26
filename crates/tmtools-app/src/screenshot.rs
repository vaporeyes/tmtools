// ABOUTME: Saves one screenshot of the window to the path in TMTOOLS_SCREENSHOT and then closes the app.
// ABOUTME: For checking the UI from scripts; eframe's own screenshot hook only covers the glow backend.

use eframe::egui;
use std::path::PathBuf;

// Frames to wait first, so layout and plot bounds have settled
const SETTLE_FRAMES: u32 = 10;

pub struct Screenshot {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

impl Screenshot {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("TMTOOLS_SCREENSHOT").map(|p| Self { path: PathBuf::from(p), frames: 0, requested: false })
    }

    /// Call once per frame
    pub fn update(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        ctx.request_repaint();
        if !self.requested && self.frames >= SETTLE_FRAMES {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(e) = save_png(&self.path, &image) {
                eprintln!("couldn't save the screenshot: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn save_png(path: &PathBuf, image: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(())
}
