// ABOUTME: Builds a map's scene and the cars' meshes on a background thread, decoding every texture they use, so the
// ABOUTME: UI keeps running while a map loads; the result is picked up once and uploaded to the GPU.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use tmassets::material::MaterialSpec;
use tmassets::texture::Texture;
use tmassets::{AssetStore, Vehicle};
use tmreplay::ReplayMap;
use tmscene::scene::{build_map, vehicle_model, MapScene, Surface};
use tmscene::vehicle::VehicleModel;

pub struct LoadedScene {
    /// Which map and vehicles this is, so it is built once per map
    pub key: String,
    pub map: Option<MapScene>,
    pub vehicles: HashMap<String, Arc<VehicleModel>>,
    pub vehicle_info: HashMap<String, Vehicle>,
    pub textures: HashMap<String, Arc<Texture>>,
    /// Why the map couldn't be built, when it couldn't
    pub error: Option<String>,
}

pub struct SceneLoad {
    pub key: String,
    receiver: Receiver<LoadedScene>,
}

/// Identifies a map and set of vehicles; another run on the same map reuses the scene
pub fn scene_key(map: &ReplayMap, vehicles: &[String]) -> String {
    let blocks = map.blocks.as_ref().map_or(0, Vec::len);
    format!("{}|{}|{}|{}|{}", map.uid.as_deref().unwrap_or(""), map.collection.as_deref().unwrap_or(""), map.decoration.as_deref().unwrap_or(""), blocks, vehicles.join(","))
}

impl SceneLoad {
    pub fn start(store: Arc<AssetStore>, map: ReplayMap, vehicles: Vec<String>) -> Self {
        let key = scene_key(&map, &vehicles);
        let (sender, receiver) = channel();
        let thread_key = key.clone();
        std::thread::spawn(move || {
            // The receiver may be gone if another map was opened meanwhile; nothing to do then
            let _ = sender.send(load(&store, &map, &vehicles, thread_key));
        });
        Self { key, receiver }
    }

    /// The finished scene, once; None while it is still loading
    pub fn poll(&self) -> Option<LoadedScene> {
        match self.receiver.try_recv() {
            Ok(scene) => Some(scene),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn load(store: &AssetStore, map: &ReplayMap, vehicles: &[String], key: String) -> LoadedScene {
    let (map_scene, mut error) = match build_map(store, map) {
        Ok(Some(scene)) => (Some(scene), None),
        Ok(None) => (None, Some("This replay's map couldn't be read, so only the run is shown.".to_owned())),
        Err(e) => (None, Some(format!("The map couldn't be built: {e}"))),
    };

    let mut vehicle_meshes = HashMap::new();
    for name in vehicles {
        match vehicle_model(store, name) {
            Ok(Some(model)) => {
                vehicle_meshes.insert(name.clone(), Arc::new(model));
            }
            Ok(None) => error = error.or(Some(format!("The asset export has no mesh for {name}"))),
            Err(e) => error = error.or(Some(e)),
        }
    }
    let vehicle_info = store.vehicles().map(|list| list.iter().map(|v| (v.name.clone(), v.clone())).collect()).unwrap_or_default();

    // Decode every texture here, off the UI thread; uploads then only copy pixels
    let mut textures = HashMap::new();
    let parts = map_scene.iter().flat_map(|m| m.parts.iter()).chain(vehicle_meshes.values().flat_map(|v| v.parts.iter()));
    for part in parts {
        if let Surface::Textured(MaterialSpec::Textured { textures: slots, .. }) = &part.surface {
            for path in [&slots.diffuse, &slots.specular, &slots.blend2, &slots.blend_intensity, &slots.blend3].into_iter().flatten() {
                if !textures.contains_key(path) {
                    if let Ok(Some(t)) = store.texture(path) {
                        textures.insert(path.clone(), t);
                    }
                }
            }
        }
    }

    LoadedScene { key, map: map_scene, vehicles: vehicle_meshes, vehicle_info, textures, error }
}
