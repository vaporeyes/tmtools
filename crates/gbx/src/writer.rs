// ABOUTME: Little-endian writer for binary Gbx data, the mirror of Reader: primitives, strings, and lookback IDs that
// ABOUTME: continue a reader's string table, so rewritten data stays consistent with what comes before and after it.

use crate::reader::{IdState, RawId};

/// The flag new strings get, as GBX.NET writes them
const NEW_STRING: u32 = 0x4000_0000;

#[derive(Debug, Default)]
pub struct Writer {
    pub bytes: Vec<u8>,
    id_version: Option<i32>,
    /// Strings with their flags, in the order they were introduced; a string's slot is its position plus one
    ids: Vec<(u32, String)>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    /// A writer continuing the lookback string table a reader had built up to this point
    pub fn with_id_state(state: &IdState) -> Self {
        Self { bytes: Vec::new(), id_version: state.version, ids: state.strings.clone() }
    }

    pub fn id_state(&self) -> IdState {
        IdState { version: self.id_version, strings: self.ids.clone() }
    }

    pub fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }

    /// Booleans are stored as 4-byte integers
    pub fn bool(&mut self, v: bool) {
        self.u32(v as u32);
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// Length-prefixed bytes
    pub fn data(&mut self, bytes: &[u8]) {
        self.i32(bytes.len() as i32);
        self.raw(bytes);
    }

    /// Length-prefixed UTF-8
    pub fn string(&mut self, s: &str) {
        self.data(s.as_bytes());
    }

    pub fn int3(&mut self, v: [i32; 3]) {
        v.iter().for_each(|&x| self.i32(x));
    }

    fn id_index(&mut self, index: u32) {
        if self.id_version.is_none() {
            self.i32(3);
            self.id_version = Some(3);
        }
        self.u32(index);
    }

    /// A string ID: a reference to the string when the table has it, otherwise the string itself, added to the table
    pub fn id_text(&mut self, text: &str) {
        self.id_flagged(NEW_STRING, text);
    }

    /// A string ID under one flag (0x40000000 or 0x80000000). Lookups are per flag: older games store a string a
    /// second time when it was introduced under the other flag.
    fn id_flagged(&mut self, new_flag: u32, text: &str) {
        if text.is_empty() {
            self.id_index(u32::MAX);
            return;
        }
        match self.ids.iter().position(|(f, s)| *f == new_flag && s == text) {
            Some(i) => {
                let flag = self.ids[i].0;
                self.id_index(flag | (i as u32 + 1));
            }
            None => {
                self.id_index(new_flag);
                self.string(text);
                self.ids.push((new_flag, text.to_owned()));
            }
        }
    }

    /// An ID as read: strings go through the table, keeping the flag they were stored with, and numbers and the
    /// empty ID are written as they were
    pub fn id_raw(&mut self, id: &RawId) {
        match &id.text {
            Some(text) => self.id_flagged(id.index & 0xC000_0000, text),
            None => self.id_index(id.index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reader;

    #[test]
    fn writes_what_the_reader_reads_back() {
        let mut w = Writer::new();
        w.u32(7);
        w.string("abc");
        w.f32(1.5);
        w.bool(true);
        w.id_text("Stadium");
        w.id_text("StadiumRoadMain");
        w.id_text("Stadium");
        w.id_raw(&RawId { index: 6, text: None });
        w.id_raw(&RawId { index: 0x8000_0000, text: Some("Nadeo".into()) });
        w.id_text("");

        let mut r = Reader::new(&w.bytes);
        assert_eq!(r.u32().unwrap(), 7);
        assert_eq!(r.string().unwrap(), "abc");
        assert_eq!(r.f32().unwrap(), 1.5);
        assert!(r.bool().unwrap());
        assert_eq!(r.id_as_string().unwrap(), "Stadium");
        assert_eq!(r.id_as_string().unwrap(), "StadiumRoadMain");
        assert_eq!(r.id_as_string().unwrap(), "Stadium");
        assert_eq!(r.id().unwrap(), "Stadium");
        assert_eq!(r.id_raw().unwrap(), RawId { index: 0x8000_0000, text: Some("Nadeo".into()) });
        assert_eq!(r.id_as_string().unwrap(), "");
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn strings_are_looked_up_per_flag() {
        // "Nadeo" under 0x80000000, then again under 0x40000000 as a new string, as Nations ESWC maps store it
        let mut w = Writer::new();
        w.id_raw(&RawId { index: 0x8000_0000, text: Some("Nadeo".into()) });
        w.id_text("Nadeo");
        w.id_raw(&RawId { index: 0x8000_0001, text: Some("Nadeo".into()) });
        assert_eq!(w.id_state().strings.len(), 2);
        let mut r = Reader::new(&w.bytes);
        for _ in 0..3 {
            assert_eq!(r.id_as_string().unwrap(), "Nadeo");
        }
        assert_eq!(&w.bytes[w.bytes.len() - 4..], &0x8000_0001u32.to_le_bytes());
    }

    #[test]
    fn continues_a_readers_string_table() {
        // A reader that has seen one string; a writer continuing from it refers to that string instead of repeating it
        let mut first = Writer::new();
        first.id_text("Alpine");
        let mut r = Reader::new(&first.bytes);
        r.id_as_string().unwrap();

        let mut w = Writer::with_id_state(&r.id_state());
        w.id_text("Alpine");
        w.id_text("Sunrise");
        assert_eq!(w.bytes.len(), 4 + 4 + 4 + "Sunrise".len(), "no version, a reference, then a new string");
        assert_eq!(&w.bytes[..4], &0x4000_0001u32.to_le_bytes());
    }
}
