// ABOUTME: Round trips the maps embedded in a folder of real replays: re-encoded unchanged they match byte for byte,
// ABOUTME: and edited copies read back with the edit. Set TMTOOLS_REPLAY_DIR; without it the test is skipped.

use std::path::PathBuf;
use tmedit::Map;
use tmreplay::{parse_challenge, parse_replay};

fn library_maps() -> Option<Vec<(String, Vec<u8>)>> {
    let dir = std::env::var_os("TMTOOLS_REPLAY_DIR").map(PathBuf::from)?;
    let mut maps = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".Replay.Gbx") {
            continue;
        }
        let replay = parse_replay(&std::fs::read(&path).unwrap()).unwrap();
        if let Some(map) = replay.embedded_map {
            maps.push((path.file_name().unwrap().to_string_lossy().into_owned(), map));
        }
    }
    Some(maps)
}

#[test]
fn unchanged_maps_write_back_byte_for_byte() {
    let Some(maps) = library_maps() else {
        eprintln!("TMTOOLS_REPLAY_DIR is not set; skipping");
        return;
    };
    assert!(!maps.is_empty());
    for (name, bytes) in &maps {
        let map = Map::parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&map.body().unwrap(), &map.file.body, "{name}: body");
        // Uncompressed, the file differs from the original only in the compression byte and the body wrapper
        let uncompressed = map.to_bytes(false).unwrap();
        let reread = gbx::GbxFile::parse(&uncompressed).unwrap();
        assert_eq!(reread.body, map.file.body, "{name}: uncompressed body");
        let stored = gbx::GbxFile::parse(&map.to_bytes(true).unwrap()).unwrap();
        assert_eq!(stored.body, map.file.body, "{name}: stored LZO body");
    }
}

#[test]
fn edits_read_back() {
    let Some(maps) = library_maps() else {
        eprintln!("TMTOOLS_REPLAY_DIR is not set; skipping");
        return;
    };
    for (name, bytes) in &maps {
        let mut map = Map::parse(bytes).unwrap();
        let before = parse_challenge(bytes).unwrap();
        // Move the last block up one level when there's room, and give the map a new identity
        let last = map.chunk.blocks.len() - 1;
        let mut cell = map.chunk.cell(last);
        cell[1] = (cell[1] + 1).min(map.chunk.size[1] - 1);
        map.chunk.set_cell(last, cell).unwrap();
        map.set_identity("tmtoolsTestUid0000000000000", "tmtools test").unwrap();

        let out = map.to_bytes(true).unwrap();
        let after = parse_challenge(&out).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((after.uid.as_str(), after.name.as_str()), ("tmtoolsTestUid0000000000000", "tmtools test"), "{name}");
        assert_eq!(after.blocks.len(), before.blocks.len());
        assert_eq!(after.blocks[last].y, cell[1], "{name}");
        let reopened = Map::parse(&out).unwrap();
        assert_eq!(reopened.chunk, map.chunk, "{name}");
        let header = reopened.file.header_reader(0x0304_3003).unwrap().read(1).unwrap()[0];
        assert!(header > 0);
    }
}

#[test]
fn new_block_types_and_authors_read_back() {
    let Some(maps) = library_maps() else {
        eprintln!("TMTOOLS_REPLAY_DIR is not set; skipping");
        return;
    };
    for (name, bytes) in &maps {
        let mut map = Map::parse(bytes).unwrap();
        assert!(map.tail.understood, "{name}: {:?}", map.tail);
        let had_clips = !map.tail.clip_spans.is_empty();
        let collection = parse_challenge(bytes).unwrap().collection;
        // A block name no map uses, so the string table gains a slot
        let index = map.chunk.add(&format!("{collection}TmtoolsProbe"), [0, 9, 0], 1, false).unwrap();
        map.set_author("tmtools_tester").unwrap();
        if had_clips {
            assert!(map.to_bytes(true).is_err(), "{name}: clips should block a table change");
            map.drop_clips();
            assert!(map.tail.ids_free(), "{name}: {:?}", map.tail);
        }
        let out = map.to_bytes(true).unwrap_or_else(|e| panic!("{name}: {e}"));
        let after = parse_challenge(&out).unwrap_or_else(|e| panic!("{name}: {e}"));
        let probe = &after.blocks[index];
        assert_eq!((probe.name.as_str(), probe.x, probe.y, probe.z, probe.dir), (format!("{collection}TmtoolsProbe").as_str(), 0, 9, 0, 1), "{name}");
        assert_eq!(after.author, "tmtools_tester", "{name}");
        let reopened = Map::parse(&out).unwrap();
        assert!(reopened.tail.ids_free(), "{name}");
        let header = reopened.file.header_reader(0x0304_3003).unwrap();
        let mut header = header;
        header.u8().unwrap();
        assert_eq!(header.ident().unwrap().author, "tmtools_tester", "{name}");
    }
}
