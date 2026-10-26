// ABOUTME: TrackMania Forever replays and maps as plain data, and the analysis built on them.
// ABOUTME: Parsing is a port of gbx-tools-3d-node's server/replay; analysis follows its viewer's study tools.

pub mod gap;
pub mod grid;
mod inputs;
mod model;
mod replay;
mod samples;
pub mod track;

pub use gbx::{Quat, Vec3};
pub use inputs::{gas_value, parse_input, steer_value};
pub use model::*;
pub use replay::{parse_challenge, parse_replay, trace_body, BodyTrace, Challenge};

/// A race time as "m:ss.hh" (hours added when needed), truncated to hundredths as the game shows it
pub fn format_race_time(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let total_hundredths = ms.unsigned_abs() / 10;
    let hundredths = total_hundredths % 100;
    let total_seconds = total_hundredths / 100;
    let (hours, minutes, seconds) = (total_seconds / 3600, (total_seconds / 60) % 60, total_seconds % 60);
    if hours > 0 {
        format!("{sign}{hours}:{minutes:02}:{seconds:02}.{hundredths:02}")
    } else {
        format!("{sign}{minutes}:{seconds:02}.{hundredths:02}")
    }
}

/// Removes TrackMania $-formatting codes from a name: colors ($f00), styles ($o, $i, ...), and links ($l[...])
pub fn strip_formatting(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('$') => out.push('$'),
            Some(h) if h.is_ascii_hexdigit() => {
                // A color takes up to three hex digits
                for _ in 0..2 {
                    if chars.peek().is_some_and(|d| d.is_ascii_hexdigit()) {
                        chars.next();
                    }
                }
            }
            Some('l' | 'L' | 'h' | 'H' | 'p' | 'P') if chars.peek() == Some(&'[') => {
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_race_times_in_hundredths() {
        assert_eq!(format_race_time(12_850), "0:12.85");
        assert_eq!(format_race_time(75_359), "1:15.35");
        assert_eq!(format_race_time(3_723_450), "1:02:03.45");
        assert_eq!(format_race_time(-250), "-0:00.25");
    }

    #[test]
    fn strips_formatting_codes() {
        assert_eq!(strip_formatting("\u{feff}$f45$s$i$f85$w$sAlex$fffbros!"), "Alexbros!");
        assert_eq!(strip_formatting("$l[http://x.y]Link$l and $$5"), "Link and $5");
        assert_eq!(strip_formatting("$F00R$0f0G"), "RG");
    }
}
