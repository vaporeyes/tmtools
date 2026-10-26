// ABOUTME: Asset names in an export: the SHA-256 of a UTF-8 key, encoded as z-base-32, and the key formats for
// ABOUTME: block, decoration, and vehicle meshes and textures. Mirrors meshHash.ts and GbxTools3D's StringExtensions.Hash.

use sha2::{Digest, Sha256};

const Z_BASE_32: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";

/// Most significant bits first; a trailing partial group is zero-padded, with no '=' padding
pub fn z_base_32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(Z_BASE_32[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(Z_BASE_32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// The file name an asset key is stored under
pub fn asset_id(key: &str) -> String {
    z_base_32(&Sha256::digest(key.as_bytes()))
}

/// .NET formats booleans as True and False
fn dotnet_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

pub fn block_mesh_key(game: &str, collection: &str, name: &str, is_ground: bool, variant: u32, sub_variant: u32) -> String {
    format!("GbxTools3D|Solid|{game}|{collection}|{name}|{}MyGuy|{variant}|{sub_variant}|PleaseDontAbuseThisThankYou:*", dotnet_bool(is_ground))
}

pub fn decoration_mesh_key(game: &str, collection: &str, solid: &str) -> String {
    format!("GbxTools3D|Decoration|{game}|{collection}|{solid}|Je te hais")
}

pub fn vehicle_mesh_key(game: &str, vehicle: &str) -> String {
    format!("GbxTools3D|Vehicle|{game}|{vehicle}|WhyDidYouNotHelpMe?")
}

pub fn texture_key(game: &str, path: &str) -> String {
    format!("GbxTools3D|Texture|{game}|{path}|PeopleOnTheBusLikeDMCA")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_z_base_32_with_partial_groups() {
        assert_eq!(z_base_32(&[]), "");
        assert_eq!(z_base_32(&[0]), "yy");
        assert_eq!(z_base_32(&[0xFF]), "9h");
        assert_eq!(z_base_32(&[0xF0, 0xBF, 0xC7]), "6n9hq");
    }

    // Reference names computed with meshHash.ts; each exists in a TMF export
    #[test]
    fn matches_the_typescript_asset_names() {
        assert_eq!(asset_id(&vehicle_mesh_key("TMF", "StadiumCar")), "f7e6a7t3ykjdgjg5ncagx8ghhqrk8geixff8w6cbbfir4bza8oqy");
        assert_eq!(asset_id(&block_mesh_key("TMF", "Stadium", "StadiumRoadMain", false, 0, 0)), "3h9ydta8x3yst37bjq7rfm9yerugodq5c5kztpssjeeqdpb7bbio");
        assert_eq!(asset_id(&texture_key("TMF", "Vehicles/Media/Texture/CoastCarSkin")), "7u7qw4ntyicxnn83xymkzu7z5oetkc9xrj8cxsmdmjdihdd3f9mo");
        assert_eq!(
            asset_id(&decoration_mesh_key("TMF", "Stadium", "Stadium/Media/Solid/Other/StadiumSkyDome")),
            "bxod8nr4eho313j458jo7u7zdc5j4j3h4nzeidihc4gq47udy4py"
        );
    }
}
