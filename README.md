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
  colored by speed and markers for braking, gas lifts, respawns, and
  checkpoints.
- **Replay Viewer** - a basic 3D wireframe view with chase, orbit, and
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
    tmtools-core/   Module trait and the shared Session (runs, focus, clock)
    inputscope/     Input Analyzer module
    telemetry/      Telemetry Graphs module
    trajmap/        Trajectory Map module
    replayview/     Replay Viewer module
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
modules analyze first. The bottom bar plays, pauses, and scrubs the shared
clock.

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
triangles, and checkpoints blue diamonds with their times. Hover the line to
read time, speed, and inputs; click it to seek there.

## Replay Viewer

A wireframe view projected on the CPU with egui's painter, so it needs no
exported meshes. Cars are oriented boxes sized like a TMF car. **Chase**
follows the focused car; **Orbit car** and **Overview** turn with a drag and
zoom with the wheel. The HUD shows time, speed, and the arrow keys lit to
the share held.

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

## Known limitations

- Only TMF-era binary Gbx files are read; TM2020 replays are not supported.
- TMUnlimiter maps hide their real blocks, so their maps show only the runs.
- Block cells are one grid square each; multi-square blocks and block shapes
  need the exported assets that gbx-tools-3d-node uses.
- Ground height comes from a small table of environment and map sizes; maps
  with unusual sizes fall back to a default and may sit at the wrong height in
  the 3D view.

## License

MIT. Class ID tables come from GBX.NET (MIT).
