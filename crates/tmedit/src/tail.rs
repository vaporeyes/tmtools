// ABOUTME: Walks the map chunks that follow the block chunk (MediaTracker clips, music, comments, password) to learn
// ABOUTME: whether anything there can refer to the lookback string table, and drops MediaTracker clips on request.

const FACADE: u32 = 0xFACA_DE01;
const SKIP: u32 = 0x534B_4950; // "PIKS"
const NULL_REF: [u8; 4] = [0xFF; 4];

/// What follows the block chunk
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tail {
    /// Every chunk was recognized and the walk ended at the body's end marker
    pub understood: bool,
    /// The data of MediaTracker chunks (0x03043021 and 0x03043026) whose clip nodes aren't null
    pub clip_spans: Vec<(usize, usize)>,
    /// Skippable chunks of unknown layout holding something that could be a reference to a string
    pub unknown_skippable: usize,
}

impl Tail {
    /// Nothing after the block chunk can refer to a string by its slot, so the block chunk may change the table
    pub fn ids_free(&self) -> bool {
        self.understood && self.clip_spans.is_empty() && self.unknown_skippable == 0
    }
}

fn word(body: &[u8], at: usize) -> Option<u32> {
    body.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// The first offset at or after `from` where `chunk` starts, for stepping over clip nodes this crate doesn't read.
/// A wrong match leaves the rest of the walk unreadable, which `understood` reports.
fn find_chunk(body: &[u8], from: usize, chunk: u32) -> Option<usize> {
    let needle = chunk.to_le_bytes();
    body.get(from..)?.windows(4).position(|w| w == needle).map(|i| from + i)
}

/// A length-prefixed string's end
fn string_end(body: &[u8], at: usize) -> Option<usize> {
    let len = word(body, at)? as usize;
    (at + 4 + len <= body.len()).then_some(at + 4 + len)
}

/// Whether any four bytes could be a lookback reference to one of the first `slots` strings: the 0x40000000 or
/// 0x80000000 flag with a slot number in range. Checked at every offset, as the chunk's layout is unknown.
fn may_refer_to_strings(data: &[u8], slots: usize) -> bool {
    data.windows(4).any(|w| {
        let v = u32::from_le_bytes(w.try_into().unwrap());
        matches!(v & 0xC000_0000, 0x4000_0000 | 0x8000_0000) && (1..=slots as u32).contains(&(v & 0x3FFF_FFFF))
    })
}

/// Walks the chunks from `from` to the body's end; `slots` is how many strings the table holds there
pub fn read_tail(body: &[u8], from: usize, slots: usize) -> Tail {
    let mut tail = Tail::default();
    let mut at = from;
    loop {
        let Some(id) = word(body, at) else { return tail };
        if id == FACADE {
            tail.understood = at + 4 == body.len();
            return tail;
        }
        if word(body, at + 4) == Some(SKIP) {
            let Some(size) = word(body, at + 8) else { return tail };
            let Some(data) = body.get(at + 12..at + 12 + size as usize) else { return tail };
            // The password chunk holds a hash and a checksum
            if id != 0x0304_3029 && may_refer_to_strings(data, slots) {
                tail.unknown_skippable += 1;
            }
            at += 12 + size as usize;
            continue;
        }
        let data = at + 4;
        let next = match id {
            // Intro, in-game, and end-race clips
            0x0304_3021 => {
                if body.get(data..data + 12) == Some(&[0xFF; 12][..]) {
                    Some(data + 12)
                } else {
                    find_chunk(body, data, 0x0304_3022).inspect(|&end| tail.clip_spans.push((data, end)))
                }
            }
            0x0304_3022 | 0x0304_302A => Some(data + 4),
            // Custom music: a pack description
            0x0304_3024 => (|| {
                let version = *body.get(data)?;
                let mut p = data + 1 + if version >= 3 { 32 } else { 0 };
                let path_len = word(body, p)?;
                p = string_end(body, p)?;
                if (path_len > 0 && version >= 1) || version >= 3 {
                    p = string_end(body, p)?;
                }
                Some(p)
            })(),
            0x0304_3025 => Some(data + 16),
            // Global clip
            0x0304_3026 => {
                if body.get(data..data + 4) == Some(&NULL_REF[..]) {
                    Some(data + 4)
                } else {
                    find_chunk(body, data, 0x0304_3028).inspect(|&end| tail.clip_spans.push((data, end)))
                }
            }
            // Thumbnail camera, then comments
            0x0304_3028 => (|| {
                let custom = word(body, data)? != 0;
                string_end(body, data + 4 + if custom { 61 } else { 0 })
            })(),
            _ => None,
        };
        match next {
            Some(next) => at = next,
            None => return tail,
        }
    }
}

/// The body with every MediaTracker clip replaced by a null reference
pub fn drop_clips(body: &[u8], tail: &Tail) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut at = 0;
    for &(start, end) in &tail.clip_spans {
        out.extend_from_slice(&body[at..start]);
        // 0x03043021 has three clip references, 0x03043026 one
        let refs = if body[start - 4..start] == 0x0304_3021u32.to_le_bytes() { 3 } else { 1 };
        (0..refs).for_each(|_| out.extend_from_slice(&NULL_REF));
        at = end;
    }
    out.extend_from_slice(&body[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn a_tail_without_clips_has_no_ids() {
        let mut body = le(&[0x0304_3021, u32::MAX, u32::MAX, u32::MAX, 0x0304_3022, 1, 0x0304_3025, 0, 0, 0, 0, 0x0304_3026, u32::MAX, 0x0304_3028, 0, 2]);
        body.extend_from_slice(b"hi");
        body.extend(le(&[0x0304_3029, SKIP, 4, 9, 0x0304_302A, 0, FACADE]));
        let tail = read_tail(&body, 0, 8);
        assert_eq!(tail, Tail { understood: true, clip_spans: vec![], unknown_skippable: 0 });
        assert!(tail.ids_free());
    }

    #[test]
    fn clips_are_found_and_dropped() {
        let body = le(&[0x0304_3021, 5, 0x0307_9000, 77, 0x0304_3022, 1, 0x0304_302A, 0, FACADE]);
        let tail = read_tail(&body, 0, 8);
        assert_eq!(tail.clip_spans, vec![(4, 16)]);
        assert!(!tail.ids_free());
        let dropped = drop_clips(&body, &tail);
        let after = read_tail(&dropped, 0, 8);
        assert!(after.ids_free(), "{after:?}");
        assert_eq!(dropped.len(), body.len());
    }

    #[test]
    fn unknown_chunks_leave_the_tail_not_understood() {
        let body = le(&[0x0304_3099, 1, FACADE]);
        assert!(!read_tail(&body, 0, 8).ids_free());
        let skippable = le(&[0x3F00_1000, SKIP, 4, 0x4000_0003, FACADE]);
        let tail = read_tail(&skippable, 0, 8);
        assert!(tail.understood && !tail.ids_free());
        // A hash with no plausible reference in it can't refer to strings
        let hash = le(&[0x3F00_1000, SKIP, 8, 0xDDF2_105B, 0xB951_34E0, FACADE]);
        assert!(read_tail(&hash, 0, 8).ids_free());
    }
}
