// ABOUTME: Assets exported from a TMF install by gbx-tools-3d-node: hashed asset names, the mesh format, textures,
// ABOUTME: material resolution, and the JSON data describing environments, blocks, decorations, and vehicles.

pub mod hash;
pub mod material;
pub mod solid;
mod store;
pub mod texture;

pub use store::{AssetStore, BlockInfo, BlockUnit, Clip, Collection, Decoration, SceneObject, UnitOffset, Variant, Vehicle, GAME};

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("{path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("{path}: {source}")]
    Json { path: String, source: serde_json::Error },
    #[error("{0}")]
    Format(String),
}

pub type Result<T> = std::result::Result<T, AssetError>;
