// ABOUTME: An asset export on disk, laid out as the web viewer's API (api/collections/TMF, api/mesh/{id}, ...), with
// ABOUTME: typed JSON data and caches of decoded meshes and textures. Safe to share between threads.

use crate::hash::{asset_id, block_mesh_key, decoration_mesh_key, texture_key, vehicle_mesh_key};
use crate::material::MaterialRecord;
use crate::solid::{decode_solid, Solid};
use crate::texture::{decode_webp, Texture};
use crate::{AssetError, Result};
use glam::{Mat4, Vec4};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The game the export covers; it names the data files
pub const GAME: &str = "TMF";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub name: String,
    pub square_size: f32,
    pub square_height: f32,
    pub default_zone_block: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub name: String,
    #[serde(default)]
    pub air_units: Vec<BlockUnit>,
    #[serde(default)]
    pub ground_units: Vec<BlockUnit>,
    #[serde(default)]
    pub air_variants: Vec<Variant>,
    #[serde(default)]
    pub ground_variants: Vec<Variant>,
    /// Set only for terrain blocks (grass, dirt, water, hills)
    pub height: Option<i32>,
    #[serde(default)]
    pub is_default_zone: bool,
    #[serde(default)]
    pub is_road: bool,
    /// Modifier this block applies to the terrain in its own column, e.g. TerrainModifierDirt
    pub terrain_modifier: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    #[serde(default)]
    pub variant: u32,
    #[serde(default)]
    pub sub_variant: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockUnit {
    #[serde(default)]
    pub offset: UnitOffset,
    /// Connectors this unit pops into a neighbouring clip block; dir is relative to the block, North when omitted
    #[serde(default)]
    pub clips: Vec<Clip>,
    /// Modifier this unit applies to the terrain below it, e.g. Fabric
    pub terrain_modifier: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct UnitOffset {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub z: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Clip {
    pub id: String,
    pub dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decoration {
    pub size: [i32; 3],
    pub base_height: Option<i32>,
    #[serde(default)]
    pub offset_block_y: bool,
    pub scene_name: String,
    #[serde(default)]
    pub scene: Vec<SceneObject>,
}

impl Decoration {
    /// Ground row for a map of this size: the decoration's base height, as gbx-tools-3d computes it
    pub fn base_height(&self) -> i32 {
        self.base_height.unwrap_or(0) + i32::from(self.offset_block_y)
    }
}

/// One object of a decoration's scene; lights and cameras have no mesh
#[derive(Debug, Clone, Deserialize)]
pub struct SceneObject {
    pub solid: Option<String>,
    /// Row-major rotation (xx..zz) and translation (tx, ty, tz); omitted fields are 0. The export also nests
    /// rotation and translation objects here, which repeat the same values and are ignored, as the web viewer does.
    #[serde(default)]
    pub location: HashMap<String, serde_json::Value>,
}

impl SceneObject {
    pub fn matrix(&self) -> Mat4 {
        let l = |k: &str| self.location.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0) as f32;
        if self.location.is_empty() {
            return Mat4::IDENTITY;
        }
        Mat4::from_cols(
            Vec4::new(l("xx"), l("yx"), l("zx"), 0.0),
            Vec4::new(l("xy"), l("yy"), l("zy"), 0.0),
            Vec4::new(l("xz"), l("yz"), l("zz"), 0.0),
            Vec4::new(l("tx"), l("ty"), l("tz"), 1.0),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vehicle {
    pub name: String,
    pub camera_fov: f32,
    /// Distance behind the car for the chase camera
    pub camera_far: f32,
    /// Height above the car for the chase camera
    pub camera_up: f32,
    /// 0 looks at the car, 1 looks straight ahead
    pub camera_look_at_factor: f32,
}

type Cache<T> = Mutex<HashMap<String, Option<Arc<T>>>>;

pub struct AssetStore {
    api: PathBuf,
    json: Mutex<HashMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
    meshes: Cache<Solid>,
    textures: Cache<Texture>,
}

impl AssetStore {
    /// Opens an export folder: either the folder holding api/, or api/ itself
    pub fn open(dir: &Path) -> Result<Self> {
        let api = if dir.join("api").is_dir() { dir.join("api") } else { dir.to_path_buf() };
        let marker = api.join("collections").join(GAME);
        if !marker.is_file() {
            return Err(AssetError::Format(format!("{} isn't an asset export: {} is missing", dir.display(), marker.display())));
        }
        Ok(Self { api, json: Mutex::default(), meshes: Mutex::default(), textures: Mutex::default() })
    }

    pub fn root(&self) -> &Path {
        &self.api
    }

    fn read(&self, relative: &str) -> Result<Option<Vec<u8>>> {
        let path = self.api.join(relative);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(AssetError::Io { path: path.display().to_string(), source }),
        }
    }

    /// JSON data, parsed once and shared
    fn json<T: DeserializeOwned + Send + Sync + 'static>(&self, relative: String) -> Result<Arc<T>> {
        if let Some(cached) = self.json.lock().expect("cache lock").get(&relative) {
            if let Ok(value) = cached.clone().downcast::<T>() {
                return Ok(value);
            }
        }
        let bytes = self.read(&relative)?.ok_or_else(|| AssetError::Format(format!("{relative} is missing from the export")))?;
        let value: Arc<T> = Arc::new(serde_json::from_slice(&bytes).map_err(|source| AssetError::Json { path: relative.clone(), source })?);
        self.json.lock().expect("cache lock").insert(relative, value.clone());
        Ok(value)
    }

    pub fn collections(&self) -> Result<Arc<Vec<Collection>>> {
        self.json(format!("collections/{GAME}"))
    }

    pub fn collection(&self, name: &str) -> Result<Option<Collection>> {
        Ok(self.collections()?.iter().find(|c| c.name == name).cloned())
    }

    pub fn block_infos(&self, collection: &str) -> Result<Arc<Vec<BlockInfo>>> {
        self.json(format!("blocks/{GAME}/{collection}"))
    }

    pub fn decorations(&self, collection: &str) -> Result<Arc<Vec<Decoration>>> {
        self.json(format!("decorations/{GAME}/{collection}"))
    }

    pub fn materials(&self) -> Result<Arc<HashMap<String, MaterialRecord>>> {
        self.json(format!("materials/{GAME}"))
    }

    pub fn vehicles(&self) -> Result<Arc<Vec<Vehicle>>> {
        self.json(format!("vehicles/{GAME}"))
    }

    /// A mesh by its asset key; None when the export has no mesh for it
    pub fn mesh(&self, key: &str) -> Result<Option<Arc<Solid>>> {
        let id = asset_id(key);
        if let Some(cached) = self.meshes.lock().expect("cache lock").get(&id) {
            return Ok(cached.clone());
        }
        let mesh = match self.read(&format!("mesh/{id}"))? {
            Some(bytes) => Some(Arc::new(decode_solid(&bytes).map_err(|e| AssetError::Format(format!("mesh {id}: {e}")))?)),
            None => None,
        };
        self.meshes.lock().expect("cache lock").insert(id, mesh.clone());
        Ok(mesh)
    }

    pub fn block_mesh(&self, collection: &str, name: &str, is_ground: bool, variant: u32, sub_variant: u32) -> Result<Option<Arc<Solid>>> {
        self.mesh(&block_mesh_key(GAME, collection, name, is_ground, variant, sub_variant))
    }

    pub fn decoration_mesh(&self, collection: &str, solid: &str) -> Result<Option<Arc<Solid>>> {
        self.mesh(&decoration_mesh_key(GAME, collection, solid))
    }

    pub fn vehicle_mesh(&self, vehicle: &str) -> Result<Option<Arc<Solid>>> {
        self.mesh(&vehicle_mesh_key(GAME, vehicle))
    }

    /// A texture by its game path, such as Stadium/Media/Texture/StadiumRoad; None when the export lacks it
    pub fn texture(&self, path: &str) -> Result<Option<Arc<Texture>>> {
        if let Some(cached) = self.textures.lock().expect("cache lock").get(path) {
            return Ok(cached.clone());
        }
        let id = asset_id(&texture_key(GAME, path));
        let texture = match self.read(&format!("texture/{id}"))? {
            Some(bytes) => Some(Arc::new(decode_webp(&bytes).map_err(|e| AssetError::Format(format!("texture {path}: {e}")))?)),
            None => None,
        };
        self.textures.lock().expect("cache lock").insert(path.to_owned(), texture.clone());
        Ok(texture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn scene_locations_are_row_major_with_translation() {
        let object: SceneObject =
            serde_json::from_str(r#"{ "solid": "x", "location": { "xz": 1, "yy": 1, "zx": -1, "tx": 10, "ty": 2, "translation": { "x": 10, "y": 2 } } }"#).unwrap();
        let p = object.matrix().transform_point3(Vec3::X);
        assert!((p - Vec3::new(10.0, 2.0, -1.0)).length() < 1e-6);
    }

    #[test]
    fn decorations_compute_their_base_height() {
        let d: Decoration = serde_json::from_str(r#"{ "size": [45, 18, 45], "baseHeight": 3, "offsetBlockY": true, "sceneName": "s" }"#).unwrap();
        assert_eq!(d.base_height(), 4);
    }

    #[test]
    fn refuses_folders_that_arent_exports() {
        assert!(AssetStore::open(Path::new("/nonexistent")).is_err());
    }
}
