# tmtools

A modular Rust + [egui](https://github.com/emilk/egui) toolset for analyzing
TrackMania Forever replays. The shell hosts independent modules in a tab bar,
and every module shares the loaded runs and one playback clock, so seeking in
any tab moves all of them:

- **Input Analyzer** - a TAS-style piano roll exact to the 10 ms physics tick,
  hold statistics, and a hold table that times each press and release against
  a second run at the same spot on the track.
- **Telemetry Graphs** - speed, gap to the focused run, steering, pedals, RPM,
  and wheel contact, stacked on one linked time axis.
- **Trajectory Map** - a top-down view of the block grid with racing lines
  colored by speed or by time gained and lost against another run, and
  markers for braking, gas lifts, respawns, checkpoints, and sector splits.
- **Run Report** - sector times for every run on a map, split at checkpoints
  and at points you place, with the best of each sector and the theoretical
  best; every braking zone and turn with entry, slowest, and exit speeds; and
  exports of inputs (CSV or a TAS script) and telemetry (CSV).
- **Replay Viewer** - the runs in 3D with the game's own meshes and textures
  (from an asset export), or a wireframe without one, with chase, orbit, and
  overview cameras and an input HUD.
- **Gbx Inspector** - the header, header chunks, body chunk tree, and a hex
  view of any Gbx file or a replay's embedded map.

## Workspace layout

```text
tmtools/
  crates/
    gbx/            Gbx container, LZO, chunk loop, node references, class IDs
    tmreplay/       replay and map parsing, input tracks, gaps, block grid
    tmassets/       asset export reading: meshes, textures, materials, data
    tmscene/        block placement, moods, and drawable map and car parts
    tmtools-core/   Module trait and the shared Session (runs, focus, clock)
    inputscope/     Input Analyzer module
    telemetry/      Telemetry Graphs module
    trajmap/        Trajectory Map module
    replayview/     Replay Viewer module
    runreport/      Run Report module
    gbxinspect/     Gbx Inspector module
    tmtools-app/    eframe shell: tabs, run list, playback bar
```

The parsing crates have no UI dependency. `gbx` and `tmreplay` are a port of
the replay parser in
[gbx-tools-3d-node](https://github.com/vaporeyes/gbx-tools-3d-node), which
follows [GBX.NET](https://github.com/BigBang1112/gbx-net).

## Requirements

- Rust 1.95 or newer (stable).
- A GPU and driver that one of wgpu's backends supports (Metal, Vulkan,
  DX12).

## Build and run

```bash
# Run the application
cargo run -p tmtools-app --release

# Open replays straight away, on a tab, at a time in milliseconds
cargo run -p tmtools-app --release -- a.Replay.Gbx b.Replay.Gbx --tab input --time 5250

# Run the test suite
cargo test --workspace
```

Open replays with **Open...** or by dropping `.Gbx` files on the window. Any
Gbx file loads; files that are not replays are still available to the
inspector.

## Game assets

The Replay Viewer can draw maps and cars with the game's own meshes and
textures. They come from an asset export made from your TrackMania Forever
install by
[gbx-tools-3d-node](https://github.com/vaporeyes/gbx-tools-3d-node)
(`npm run export -- <game dir> <export dir>`); tmtools reads the same folder
the web viewer serves and never bundles any game files.

Choose the folder with **Choose asset folder...** at the foot of the side
panel. It is remembered between launches; `TMTOOLS_ASSETS_DIR` or
`--assets DIR` override it. Without an export the viewer draws wireframes.

## Shared controls

The left panel lists loaded files and their runs (ghosts). The checkbox shows
or hides a run, and clicking a run's name makes it the focused run that
modules analyze first. The bottom bar plays, pauses, steps one tick at a
time, and scrubs the shared clock; its timeline marks the focused run's
checkpoints.

| Key     | Action                   |
| ------- | ------------------------ |
| `Space` | Play or pause            |
| `,`     | Back one tick (10 ms)    |
| `.`     | Forward one tick (10 ms) |
| `Home`  | Back to the start        |

## Input Analyzer

TMF records inputs as changes at 10 ms ticks; this module reads those events,
not the 100 ms car samples, so every press and release lands on its exact
tick.

- The piano roll has rows for Left, Right, Gas, and Brake. Bars fill to the
  share held, so analog steering shows its amount. Each hold is labeled with
  its length when it fits.
- Drag to pan, scroll to zoom, and click to seek to a tick. **Ticks** zooms in
  until each tick is a column; **Fit run** shows the whole run.
- Respawns are purple lines and checkpoints blue lines.
- **Align by track position** draws the compared run's rows where its car
  was on the track instead of at its race time, so both drivers' inputs for
  the same corner line up; hold labels keep their real lengths, and hovering
  shows the compared run's inputs at that spot along with its own clock.
- **Compare with** adds a second run's rows. The hold table then shows, for
  each press and release, when the other run did the same thing at the spot
  where this run did it: for example "0.04 earlier" means the other driver
  braked four ticks before reaching that point.

## Telemetry Graphs

Plots share their time axis and cursor. Steering and pedals are drawn from
the exact inputs as step lines; the dashed steering line is what the car's
samples recorded. The gap plot is positive when a run is behind the focused
run at the same place on the track. Click any plot to seek.

## Trajectory Map

Blocks are drawn as grid cells shaded by height; block shapes are not
modeled. The focused run's line is colored from blue (slow) to red (fast).
Brake presses are filled red dots, releases red rings, gas lifts orange
triangles, checkpoints blue diamonds with their times, and placed sector splits
white squares labeled with the sector they start. Hover the line to
read time, speed, and inputs; click it to seek there.

**Color by** switches the focused line from speed to time against another run
on the same map: green where the focused run gains, red where it loses, as
the slope of the gap between them over 0.4 s (full color at the 95th
percentile rate, shown in the legend). Gray stretches are where the lines are
too far apart to compare. Hovering then also shows the gap and its trend.

## Run Report

The sector table covers the focused run and every visible run on the same
map. Sectors end at each checkpoint and at each split placed with **Place
splits** on the Trajectory Map (click the focused line to place one,
right-click a split to remove it; splits are kept per map while the app runs).
Each run's time at a placed split is when it passes that point, matched in
order along the track. The best time of each sector is highlighted, others
show what they lose to it, and the **Best** row adds up to the theoretical
best. Clicking a time jumps to the start of that sector for that run.

**Braking and turns** lists the focused run's braking zones and turns, found
in its tick-exact inputs: inputs of one kind less than 0.25 s apart count as
one event, so a series of taps is one turn. Each row shows when it starts,
how long it lasts in seconds and ticks, the speed at its start, slowest
point, and end, the speed lost, ticks of braking within it, and how it was
steered (one hold, taps with the share held, or the analog peak). Click the
start or the slowest point to jump there; filter by braking and left or
right turns.

**Export the focused run** saves, through a file dialog:

- **Inputs CSV** - every recorded input event: `time_ms`, `tick`, `input`,
  and `value` (1 or 0 for buttons, the raw amount for analog inputs).
- **TAS script** - the inputs in TMInterface's script style: digital holds as
  `start-end press up|down|left|right` ranges in seconds (a hold still down at
  the finish is a lone `press`), analog steering and gas as raw `steer` and
  `gas` values, and respawns as `press enter`.
- **Telemetry CSV** - one row per sample: position, speed, gas, brake, and
  steering as the car recorded them, RPM, turbo, and how many wheels slide and
  touch the ground.

## Replay Viewer

With an asset export set, the viewer draws the map and cars as the web viewer
in gbx-tools-3d-node does: blocks placed by the same rules (variants,
rotations, default terrain, clips, terrain modifiers), the decoration around
the track, the vehicle meshes with their wheels spinning, steering, and riding
the suspension from the ghost's samples, real textures with layered grass,
normal maps, Phong highlights from specular maps, and the map's mood (sky
gradient, sun, ambient light, and fog). It renders with wgpu into a
4x multisampled image; the map loads on a background thread and the wireframe
shows until it is ready. **Game textures** switches back to the wireframe.

Racing lines are drawn a few pixels wide at any distance (bolder for the
focused run), and yellow posts mark where the focused run took each
checkpoint; both can be switched off. The cars of runs you aren't following
are drawn as gray see-through ghosts, as the web viewer does (**Ghost other
cars**, with a slider for how opaque).

**Chase** uses each vehicle's own chase camera settings from the export;
**Orbit car** and **Overview** turn with a drag and zoom with the wheel. The
HUD shows time, speed, and the arrow keys lit to the share held.

## Gbx Inspector

Shows the header fields, header chunks, external references, and the body's
chunks in the order the readers met them, nested under the node that holds
them. Chunks are labeled with their class name and number and their raw ID.
Grayed chunks were skipped without being read. Replays stop at the
MediaTracker clip on purpose, and the walk notes where. Select any chunk to
see its bytes in hex.

## Testing

`cargo test --workspace` covers the Gbx reader, LZO decoder, chunk loop,
input decoding, sample unpacking, input tracks and hold statistics, gaps and
press matching, the block grid, the session clock, and each module's view
math.

`crates/tmassets/tests/export.rs` decodes every mesh and texture of a real
export and checks that the names built from its data resolve. Set
`TMTOOLS_ASSETS_DIR` to the export to run it.

`crates/tmscene/tests/library.rs` checks block placement against the web
viewer's on real maps and builds every map's scene from the export; it needs
both `TMTOOLS_REPLAY_DIR` and `TMTOOLS_ASSETS_DIR`.

`crates/tmreplay/tests/library.rs` compares the parser against the
TypeScript parser's output for a folder of real replays. Set
`TMTOOLS_REPLAY_DIR` to that folder to run it; without it the test is
skipped.

## Screenshots from scripts

Set `TMTOOLS_SCREENSHOT` to a PNG path and the app saves one screenshot after
the layout settles, then exits:

```bash
TMTOOLS_SCREENSHOT=/tmp/shot.png cargo run -p tmtools-app -- a.Replay.Gbx --tab map
```

`TMTOOLS_SCREENSHOT_DELAY_MS` waits longer first, for example while a map
loads.

## Known limitations

- Only TMF-era binary Gbx files are read; TM2020 replays are not supported.
- TMUnlimiter maps hide their real blocks, so their maps show only the runs.
- Without an asset export, block cells are one grid square each and ground
  height comes from a small table of environment and map sizes.
- The textured view leaves out the faint specular term of three.js's
  standard material and night lightmaps; materials the web viewer
  draws as see-through are drawn the same way here.

## License

MIT. Class ID tables come from GBX.NET (MIT).
