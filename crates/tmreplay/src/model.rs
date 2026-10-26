// ABOUTME: The parsed replay as plain data: the map with its blocks, and each ghost's samples and recorded inputs.
// ABOUTME: Units follow the game: milliseconds, world units (32 per Stadium block), km/h, radians.

use gbx::{Quat, Vec3};

#[derive(Debug, Clone, Default)]
pub struct Replay {
    pub map: ReplayMap,
    pub ghosts: Vec<Ghost>,
    /// The map file embedded in the replay, when it has one
    pub embedded_map: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct ReplayMap {
    pub uid: Option<String>,
    pub name: Option<String>,
    pub author: Option<String>,
    /// Environment from the replay header, such as "Stadium"
    pub environment: Option<String>,
    pub collection: Option<String>,
    pub size: Option<[i32; 3]>,
    pub decoration: Option<String>,
    /// None when the map couldn't be read, or it is a TMUnlimiter map whose real blocks are hidden
    pub blocks: Option<Vec<MapBlock>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapBlock {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// 0 north, 1 east, 2 south, 3 west
    pub dir: u8,
    pub is_ground: bool,
    pub variant: u32,
    pub sub_variant: u32,
    pub is_clip: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Ghost {
    pub nickname: Option<String>,
    pub login: Option<String>,
    pub vehicle: Option<String>,
    pub race_time_ms: Option<i32>,
    pub checkpoints: Vec<Checkpoint>,
    pub sample_period_ms: Option<i32>,
    pub samples: Vec<Sample>,
    pub inputs: Vec<Input>,
}

impl Ghost {
    /// The nickname without formatting codes, else the login
    pub fn display_name(&self) -> String {
        match (&self.nickname, &self.login) {
            (Some(n), _) if !crate::strip_formatting(n).trim().is_empty() => crate::strip_formatting(n),
            (_, Some(l)) => l.clone(),
            _ => "Unnamed ghost".into(),
        }
    }

    /// How long the run lasts: its race time, else its last sample or input
    pub fn duration_ms(&self) -> i32 {
        self.race_time_ms
            .or_else(|| self.samples.last().map(|s| s.time_ms))
            .or_else(|| self.inputs.last().map(|i| i.time_ms))
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checkpoint {
    pub time_ms: Option<i32>,
    pub stunts_score: i32,
}

/// One car state, recorded every sample period (100 ms in TMF)
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    pub time_ms: i32,
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub speed_kmh: f32,
    /// FL, FR, RR, RL
    pub wheel_rotation: [f32; 4],
    /// Front wheel angle in radians
    pub steer_front: f32,
    /// FL, FR, RR, RL suspension, -2 to 2
    pub dampen_len: [f32; 4],
    /// 0 to 1
    pub gas: f32,
    pub brake: f32,
    /// -1 to 1
    pub steer: f32,
    pub rpm: f32,
    pub turbo: f32,
    pub dirt_blend: f32,
    pub horn: u8,
    /// Per-wheel bitmasks: 1 FL, 2 FR, 4 RR, 8 RL
    pub sliding: u8,
    pub grounded: u8,
}

/// A recorded input event; the game records changes only, at 10 ms ticks
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub time_ms: i32,
    /// GBX.NET's input type name: Accelerate, Brake, SteerLeft, SteerRight, Steer, Gas, Respawn, ...
    pub name: String,
    pub value: InputValue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputValue {
    Pressed(bool),
    /// Steering and analog pedals: -65536 to 65536 (full left to full right for steering)
    Analog(i32),
    None,
}
