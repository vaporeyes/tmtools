// ABOUTME: Decodes exported textures (WebP) into RGBA pixels ready to upload to the GPU.
// ABOUTME: Images without alpha get an opaque alpha channel so every texture has the same layout.

use crate::{AssetError, Result};
use std::io::Cursor;

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA, 4 bytes per pixel
    pub rgba: Vec<u8>,
}

pub fn decode_webp(data: &[u8]) -> Result<Texture> {
    let err = |e: image_webp::DecodingError| AssetError::Format(format!("texture doesn't decode: {e}"));
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(data)).map_err(err)?;
    let (width, height) = decoder.dimensions();
    let mut pixels = vec![0; decoder.output_buffer_size().ok_or_else(|| AssetError::Format("texture is too large".into()))?];
    decoder.read_image(&mut pixels).map_err(err)?;
    let rgba = if decoder.has_alpha() { pixels } else { pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    Ok(Texture { width, height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_data_that_isnt_webp() {
        assert!(decode_webp(b"not a texture").is_err());
    }
}
