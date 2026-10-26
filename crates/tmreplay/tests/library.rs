// ABOUTME: Parity test against gbx-tools-3d-node's TypeScript replay parser over a folder of real replays.
// ABOUTME: Set TMTOOLS_REPLAY_DIR to the folder library-expected.tsv was made from; without it the test is skipped.

use std::path::PathBuf;
use tmreplay::{parse_replay, InputValue};

fn input_text(input: Option<&tmreplay::Input>) -> String {
    match input {
        None => String::new(),
        Some(i) => {
            let value = match i.value {
                InputValue::Pressed(p) => p.to_string(),
                InputValue::Analog(v) => v.to_string(),
                InputValue::None => "null".into(),
            };
            format!("{}:{}={value}", i.time_ms, i.name)
        }
    }
}

#[test]
fn matches_the_typescript_parser() {
    let Some(dir) = std::env::var_os("TMTOOLS_REPLAY_DIR").map(PathBuf::from) else {
        eprintln!("TMTOOLS_REPLAY_DIR is not set; skipping the replay parity test");
        return;
    };
    let expected = include_str!("fixtures/library-expected.tsv");
    let mut checked = 0;
    for line in expected.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let bytes = std::fs::read(dir.join(cols[0])).unwrap_or_else(|e| panic!("{}: {e}", cols[0]));
        let replay = parse_replay(&bytes).unwrap_or_else(|e| panic!("{}: {e}", cols[0]));
        let ghost = &replay.ghosts[cols[1].parse::<usize>().unwrap()];
        let n = ghost.samples.len();

        let at10 = ghost.samples.get(n.saturating_sub(1).min(10));
        let actual = [
            ghost.race_time_ms.map_or("null".into(), |t| t.to_string()),
            ghost.checkpoints.iter().map(|c| c.time_ms.map_or("null".into(), |t| t.to_string())).collect::<Vec<_>>().join(","),
            n.to_string(),
            ghost.inputs.len().to_string(),
            input_text(ghost.inputs.first()),
            input_text(ghost.inputs.last()),
            at10.map_or(String::new(), |s| format!("{:.3},{:.3},{:.3}", s.position.x, s.position.y, s.position.z)),
            at10.map_or(String::new(), |s| format!("{:.2}", s.speed_kmh)),
            replay.map.blocks.as_ref().map_or("none".into(), |b| b.len().to_string()),
            format!("{:?}", replay.map.name.as_deref().unwrap_or("")),
        ];
        assert_eq!(actual.as_slice(), &cols[2..], "{}", cols[0]);
        checked += 1;
    }
    assert!(checked > 0);
}
