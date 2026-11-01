// ABOUTME: tmgen command: generates a TMF Stadium platform/tech map from a folder of replays and writes it as a
// ABOUTME: .Challenge.Gbx. Run with --help for the options.

use std::path::PathBuf;
use tmgen::generate::{is_hole, is_speedup};
use tmgen::{generate, pillars, read_library, write_map, Pillars, Settings};

const USAGE: &str = "tmgen: generate a TMF Stadium platform/tech map

  --library DIR    folder of .Replay.Gbx files to learn from (default: $TMTOOLS_REPLAY_DIR)
  --assets DIR     the game asset export (default: $TMTOOLS_ASSETS_DIR)
  --out FILE       where to write the map (default: \"tmgen <seed>.Challenge.Gbx\")
  --seed N         the same seed gives the same map (default: 1)
  --length N       pieces before the finish, roughly (default: 30)
  --checkpoints N  pieces between checkpoints (default: 8)
  --speedups N     turbo pads to spread along the track (default: 2)
  --holes N        floor holes to spread along the track (default: 1)
  --pillars KIND   none, tube, or inflatable:V with V a look from 0 to 8 (default: tube)
  --mood NAME      Day, Sunset, Sunrise, or Night, when the library has a base map in it (default: any)
  --name TEXT      map name (default: \"tmgen <seed>\")
  --author LOGIN   your TMF login, so you can open the map in the editor without paying coppers";

fn main() {
    if let Err(e) = run() {
        eprintln!("tmgen: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut settings = Settings::default();
    let env_dir = |key: &str| std::env::var_os(key).map(PathBuf::from);
    let (mut library, mut assets) = (env_dir("TMTOOLS_REPLAY_DIR"), env_dir("TMTOOLS_ASSETS_DIR"));
    let (mut out, mut mood, mut name, mut author) = (None, None, None, String::new());
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let number = |v: String| v.parse::<u64>().map_err(|_| format!("{v} isn't a number"));
        match arg.as_str() {
            "--library" => library = Some(value()?.into()),
            "--assets" => assets = Some(value()?.into()),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--seed" => settings.seed = number(value()?)?,
            "--length" => settings.length = number(value()?)? as usize,
            "--checkpoints" => settings.checkpoint_every = number(value()?)?.max(1) as usize,
            "--speedups" => settings.speedups = number(value()?)? as usize,
            "--holes" => settings.holes = number(value()?)? as usize,
            "--pillars" => {
                settings.pillars = match value()?.as_str() {
                    "none" => Pillars::None,
                    "tube" => Pillars::Tube,
                    other => match other.strip_prefix("inflatable") {
                        Some(look) => Pillars::Inflatable(look.trim_start_matches(':').parse().unwrap_or(0).min(8)),
                        None => return Err(format!("unknown pillars {other}: use none, tube, or inflatable:V")),
                    },
                }
            }
            "--mood" => mood = Some(value()?),
            "--name" => name = Some(value()?),
            "--author" => author = value()?,
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    let library = library.ok_or("no replay folder: pass --library or set TMTOOLS_REPLAY_DIR")?;
    let assets = assets.ok_or("no asset export: pass --assets or set TMTOOLS_ASSETS_DIR")?;

    let store = tmassets::AssetStore::open(&assets).map_err(|e| e.to_string())?;
    let infos = store.block_infos("Stadium").map_err(|e| e.to_string())?;
    let lib = read_library(&library, &infos).map_err(|e| format!("{}: {e}", library.display()))?;
    println!("Learned from {} runs in {} replays: {} block connections", lib.model.runs, lib.replays, lib.model.links.values().map(|l| l.len()).sum::<usize>());

    let (base_mood, base) = lib
        .bases
        .iter()
        .find(|(m, _)| mood.as_deref().is_none_or(|want| m.eq_ignore_ascii_case(want)))
        .ok_or(match &mood {
            Some(m) => format!("no TMF Stadium map in the library has the {m} mood to build on"),
            None => "no TMF Stadium map in the library can be built on".to_owned(),
        })?;
    let track = generate(&lib.model, settings).ok_or("no track found for these settings; try another seed or a shorter length")?;
    let name = name.unwrap_or(format!("tmgen {}", settings.seed));
    let supports = pillars(&lib.model, &track, settings);
    let blocks: Vec<_> = track.iter().chain(&supports).cloned().collect();
    let bytes = write_map(base, &blocks, &name, &author, settings.seed)?;
    let out = out.unwrap_or(PathBuf::from(format!("{name}.Challenge.Gbx")));
    std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;

    let count = |f: fn(&str) -> bool| track.iter().filter(|p| f(&p.name)).count();
    let top = track.iter().map(|p| p.anchor[1]).max().unwrap_or(1) - 1;
    println!(
        "Wrote {}: {} pieces, {} checkpoints, {} speedups, {} floor holes, {} pillar blocks, up to {top} levels high, {base_mood} mood",
        out.display(),
        track.len(),
        count(tmgen::generate::is_checkpoint),
        count(is_speedup),
        count(is_hole),
        supports.len()
    );
    if author.is_empty() {
        println!("No --author given, so the map keeps its base map's author and the game asks for coppers to edit it");
    }
    Ok(())
}
