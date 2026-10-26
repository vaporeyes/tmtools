// ABOUTME: Text helpers for the inspector: one hex dump row (offset, 16 bytes, ASCII) and a chunk's readable label
// ABOUTME: from its class name and chunk number.

use gbx::class_ids::class_name;

pub const ROW_BYTES: usize = 16;

/// One hex dump row: the offset, the bytes in hex (short rows padded), and printable ASCII
pub fn hex_row(offset: usize, bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(ROW_BYTES * 3 + 1);
    for i in 0..ROW_BYTES {
        if i == 8 {
            hex.push(' ');
        }
        match bytes.get(i) {
            Some(b) => hex.push_str(&format!("{b:02X} ")),
            None => hex.push_str("   "),
        }
    }
    let ascii: String = bytes.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' }).collect();
    format!("{offset:08X}  {hex} {ascii}")
}

/// "CGameCtnGhost 0x019" for chunk 0x03092019, or the bare ID when the class is unknown
pub fn chunk_label(chunk_id: u32) -> String {
    match class_name(chunk_id & 0xFFFF_F000) {
        Some(name) => format!("{name} 0x{:03X}", chunk_id & 0xFFF),
        None => format!("0x{chunk_id:08X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_full_and_short_rows() {
        let row = hex_row(0x10, b"GBX\x06\x00BUCR");
        assert!(row.starts_with("00000010  47 42 58 06 00 42 55 43  52 "));
        assert!(row.ends_with(" GBX..BUCR"));
        assert_eq!(hex_row(0, &[0u8; 16]).len(), hex_row(0, &[0u8; 3]).len() - 3 + 16);
    }

    #[test]
    fn labels_chunks_by_class() {
        assert_eq!(chunk_label(0x0309_2019), "CGameCtnGhost 0x019");
        assert_eq!(chunk_label(0xDEAD_B001), "0xDEADB001");
    }
}
