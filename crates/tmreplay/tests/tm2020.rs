// ABOUTME: Reads a TM2020 ghost file (CGameCtnGhost with a CPlugEntRecordData recording and bit-packed inputs).
// ABOUTME: The fixture is a 21.29 s run with five checkpoints, recorded every 50 ms.

use tmreplay::{parse_replay, InputValue};

#[test]
fn reads_a_tm2020_ghost_file() {
    let replay = parse_replay(include_bytes!("fixtures/tm2020.Ghost.Gbx")).unwrap();
    assert!(replay.map.blocks.is_none());
    let [ghost] = &replay.ghosts[..] else { panic!("expected one ghost") };
    assert_eq!(ghost.display_name(), "dreams---");
    assert_eq!(ghost.vehicle.as_deref(), Some("CarSport"));
    assert_eq!(ghost.race_time_ms, Some(21290));
    assert_eq!(ghost.checkpoints.last().and_then(|c| c.time_ms), Some(21290));

    // The car's states, from the start block to past the finish
    assert_eq!((ghost.sample_period_ms, ghost.samples.len()), (Some(50), 427));
    let first = ghost.samples[0];
    assert_eq!(first.time_ms, 40);
    assert!((first.position.x - 1840.0).abs() < 0.1 && (first.position.z - 1136.0).abs() < 0.1, "{:?}", first.position);
    assert!(ghost.samples.last().unwrap().time_ms >= 21290);
    let top = ghost.samples.iter().map(|s| s.speed_kmh).fold(0.0, f32::max);
    assert!((330.0..345.0).contains(&top), "{top}");
    // Airborne on a jump: no wheel touches the ground
    assert!(ghost.samples.iter().any(|s| s.grounded == 0) && first.grounded == 0b1111);

    // Inputs start in the countdown, holding the gas through the run
    let gas = ghost.inputs.iter().find(|i| i.name == "Accelerate").unwrap();
    assert_eq!((gas.time_ms, gas.value), (-1510, InputValue::Pressed(true)));
    assert!(ghost.inputs.iter().any(|i| i.name == "Steer" && matches!(i.value, InputValue::Analog(v) if v < 0)));
    assert!(ghost.inputs.iter().all(|i| i.time_ms < 21290));
    // A clean run: the countdown's respawn bit isn't a respawn
    assert!(!ghost.inputs.iter().any(|i| i.name == "Respawn"));
}
