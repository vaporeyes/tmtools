// ABOUTME: Lighting moods for TMF maps (Sunrise, Day, Sunset, Night) per environment, and how to pick one.
// ABOUTME: Port of moods.ts: Stadium colors from its mood files, other environments' sky and ambient from their panoramas.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoodName {
    Sunrise,
    Day,
    Sunset,
    Night,
}

/// Colors are sRGB, as the game's mood files give them
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mood {
    pub name: MoodName,
    /// Sky from zenith to horizon
    pub sky: [[u8; 3]; 5],
    pub sun: [u8; 3],
    pub ambient: [u8; 3],
    /// Degrees above the horizon, and compass direction the light comes from (0 is +Z)
    pub sun_elevation: f32,
    pub sun_azimuth: f32,
    pub sun_intensity: f32,
    pub ambient_intensity: f32,
}

const fn hex(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

const fn sky(c: [u32; 5]) -> [[u8; 3]; 5] {
    [hex(c[0]), hex(c[1]), hex(c[2]), hex(c[3]), hex(c[4])]
}

const SUNRISE: Mood = Mood {
    name: MoodName::Sunrise,
    sky: sky([0x556a86, 0x779bc2, 0x8ea7c0, 0xafb0a9, 0xf7ebd7]),
    sun: hex(0xffd996),
    ambient: hex(0x8695a5),
    sun_elevation: 14.0,
    sun_azimuth: 70.0,
    sun_intensity: 2.4,
    ambient_intensity: 2.2,
};

// Day uses the game's time-of-day ramps; these are the 14:00 values
const DAY: Mood = Mood {
    name: MoodName::Day,
    sky: sky([0x3e699f, 0x508ac5, 0x659dcb, 0x8fbddc, 0xd5e5f0]),
    sun: hex(0xffffff),
    ambient: hex(0x626c74),
    sun_elevation: 55.0,
    sun_azimuth: 210.0,
    sun_intensity: 2.6,
    ambient_intensity: 3.2,
};

const SUNSET: Mood = Mood {
    name: MoodName::Sunset,
    sky: sky([0x374675, 0x7b94c0, 0xc9d5e1, 0xefc09a, 0xb56368]),
    sun: hex(0xff903a),
    ambient: hex(0x97869c),
    sun_elevation: 10.0,
    sun_azimuth: 290.0,
    sun_intensity: 2.4,
    ambient_intensity: 2.0,
};

// The game's night light is baked into lightmaps, which the export lacks; a dim, cool moonlight stands in
const NIGHT: Mood = Mood {
    name: MoodName::Night,
    sky: sky([0x000000, 0x101011, 0x1e1e25, 0x23232a, 0x25252d]),
    sun: hex(0x8a9cc4),
    ambient: hex(0x363536),
    sun_elevation: 40.0,
    sun_azimuth: 160.0,
    sun_intensity: 0.9,
    ambient_intensity: 3.0,
};

/// Sky and ambient for environments other than Stadium: each mood's SkyPanoramic.dds rows and AmbGrad.dds middle
fn environment_colors(environment: &str, mood: MoodName) -> Option<([u32; 5], u32)> {
    use MoodName::*;
    Some(match (environment, mood) {
        ("Alpine", Sunrise) => ([0x7b98b2, 0x819db5, 0x96abbc, 0x959aa1, 0x89878b], 0x82858f),
        ("Alpine", Day) => ([0xe8f0fd, 0xd0dcef, 0x93a7cc, 0x98adcf, 0xa6b8cd], 0x5e6165),
        ("Alpine", Sunset) => ([0x9f94b9, 0xb7aabc, 0xbdaeba, 0xbfa9a9, 0xa88e91], 0x8a8493),
        ("Alpine", Night) => ([0x0e1323, 0x0f1627, 0x172036, 0x303b52, 0x35353b], 0x323237),
        ("Bay", Sunrise) => ([0x7b98b2, 0x819db5, 0x96abbc, 0x959aa1, 0x89878b], 0x444958),
        ("Bay", Day) => ([0x3c5185, 0x7284a7, 0x7b94b6, 0x9aaec6, 0x9faab8], 0x5e6165),
        ("Bay", Sunset) => ([0x72759e, 0x9e8999, 0xab98a2, 0xaf9c99, 0x80716b], 0x5c516d),
        ("Bay", Night) => ([0x0e1323, 0x0f1627, 0x172036, 0x303b52, 0x35353b], 0x323237),
        ("Coast", Sunrise) => ([0x5b6782, 0x6d7d99, 0x8993a3, 0x96999a, 0x7f7e84], 0x7e8392),
        ("Coast", Day) => ([0x5a8bba, 0x60a1d1, 0x7fb4d8, 0xb3d3e8, 0xe1ecf3], 0x5e6165),
        ("Coast", Sunset) => ([0x74809c, 0x8e91a7, 0xb0a0a7, 0xcba591, 0xd79161], 0x8c7979),
        ("Coast", Night) => ([0x080b17, 0x0c101e, 0x0d1221, 0x131723, 0x090b11], 0x424246),
        ("Island", Sunrise) => ([0x555e76, 0x5a6379, 0x6f7588, 0x818593, 0x928987], 0x444958),
        ("Island", Day) => ([0x768cad, 0x566f95, 0x697da0, 0x859bb5, 0xbcbdba], 0x5e6165),
        ("Island", Sunset) => ([0x7a92c0, 0xa981a0, 0xb7788c, 0xd0817c, 0xc7816f], 0x5c516d),
        ("Island", Night) => ([0x0b1123, 0x0e172e, 0x131d36, 0x1d273b, 0x181822], 0x3d3d41),
        ("Rally", Sunrise) => ([0x8991b5, 0x979bba, 0xa2a3b9, 0xb7b4c2, 0xa2a9c6], 0x9fa3ae),
        ("Rally", Day) => ([0xd3e1ec, 0xcedcea, 0xc4d2e2, 0xb2c1d1, 0xb0bdca], 0x5e6165),
        ("Rally", Sunset) => ([0xb3a6bc, 0xb8aab8, 0xb5abb8, 0xc2afab, 0xb6a19d], 0x918a8a),
        ("Rally", Night) => ([0x080b17, 0x0c101e, 0x0d1221, 0x131723, 0x090b11], 0x424246),
        ("Speed", Sunrise) => ([0xa4b3d2, 0xb3c1db, 0xcbcad2, 0xcdcfd0, 0xd1ae9d], 0x9599a6),
        ("Speed", Day) => ([0x8794b6, 0x9ca7c3, 0x8996b8, 0x93a3c3, 0xb2b3b6], 0x5e6165),
        ("Speed", Sunset) => ([0x798cb0, 0x8392b6, 0x8c8ba4, 0x898495, 0x867280], 0xa39595),
        ("Speed", Night) => ([0x0b1123, 0x0e172e, 0x131d36, 0x1d273b, 0x181822], 0x3d3d41),
        _ => return None,
    })
}

/// Mood for a map's decoration name such as "Sunset" in its environment; unknown names get Day
pub fn mood_for(decoration: Option<&str>, environment: Option<&str>) -> Mood {
    let lower = decoration.unwrap_or("").to_ascii_lowercase();
    // The first mood word in the name wins, as the web viewer's regex does
    let found = ["sunrise", "day", "sunset", "night"].iter().filter_map(|w| lower.find(w).map(|at| (at, *w))).min();
    let mut mood = match found.map(|(_, w)| w) {
        Some("sunrise") => SUNRISE,
        Some("sunset") => SUNSET,
        Some("night") => NIGHT,
        _ => DAY,
    };
    if let Some((sky_colors, ambient)) = environment.and_then(|e| environment_colors(e, mood.name)) {
        mood.sky = sky(sky_colors);
        mood.ambient = hex(ambient);
    }
    mood
}

/// An sRGB byte as a linear value, for lighting math
pub fn linear(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    })
}

impl Mood {
    /// Unit vector toward the sun
    pub fn sun_direction(&self) -> [f32; 3] {
        let (e, a) = (self.sun_elevation.to_radians(), self.sun_azimuth.to_radians());
        [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_mood_named_in_the_decoration() {
        assert_eq!(mood_for(Some("StadiumSunset"), Some("Stadium")).name, MoodName::Sunset);
        assert_eq!(mood_for(Some("Night"), None).name, MoodName::Night);
        assert_eq!(mood_for(Some("Base45x45"), None).name, MoodName::Day);
        assert_eq!(mood_for(None, None).name, MoodName::Day);
    }

    #[test]
    fn other_environments_bring_their_own_sky() {
        let island = mood_for(Some("Sunrise"), Some("Island"));
        assert_eq!(island.sky[0], hex(0x555e76));
        assert_eq!(island.sun, SUNRISE.sun);
        assert_eq!(mood_for(Some("Sunrise"), Some("Stadium")).sky, SUNRISE.sky);
    }

    #[test]
    fn linearizes_srgb() {
        assert_eq!(linear([0, 255, 255]), [0.0, 1.0, 1.0]);
        assert!((linear([128, 0, 0])[0] - 0.2158).abs() < 1e-3);
    }
}
