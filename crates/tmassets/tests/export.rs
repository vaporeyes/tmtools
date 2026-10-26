// ABOUTME: Reads a whole real asset export: every mesh and texture decodes, every JSON file parses, and the names
// ABOUTME: built from the data resolve. Set TMTOOLS_ASSETS_DIR to the export; without it the test is skipped.

use std::path::PathBuf;
use tmassets::material::{resolve_material, MaterialSpec};
use tmassets::solid::{decode_solid, mesh_parts};
use tmassets::texture::decode_webp;
use tmassets::AssetStore;

fn export_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("TMTOOLS_ASSETS_DIR").map(PathBuf::from);
    if dir.is_none() {
        eprintln!("TMTOOLS_ASSETS_DIR is not set; skipping the export test");
    }
    dir
}

#[test]
fn every_mesh_and_texture_decodes() {
    let Some(dir) = export_dir() else { return };
    let store = AssetStore::open(&dir).unwrap();

    let mut meshes = 0;
    for entry in std::fs::read_dir(store.root().join("mesh")).unwrap() {
        let path = entry.unwrap().path();
        let solid = decode_solid(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for part in mesh_parts(&solid.root) {
            let v = &part.visual;
            assert!(v.indices.iter().all(|&i| (i as usize) < v.positions.len()), "{}: index out of range", path.display());
        }
        meshes += 1;
    }

    let mut textures = 0;
    for entry in std::fs::read_dir(store.root().join("texture")).unwrap() {
        let path = entry.unwrap().path();
        let t = decode_webp(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(t.rgba.len(), (t.width * t.height * 4) as usize);
        textures += 1;
    }
    eprintln!("decoded {meshes} meshes and {textures} textures");
    assert!(meshes > 0 && textures > 0);
}

#[test]
fn data_parses_and_names_resolve() {
    let Some(dir) = export_dir() else { return };
    let store = AssetStore::open(&dir).unwrap();

    for collection in store.collections().unwrap().iter() {
        let blocks = store.block_infos(&collection.name).unwrap();
        let decorations = store.decorations(&collection.name).unwrap();
        eprintln!("{}: {} blocks, {} decorations", collection.name, blocks.len(), decorations.len());
    }

    let stadium = store.block_infos("Stadium").unwrap();
    let road = stadium.iter().find(|b| b.name == "StadiumRoadMain").unwrap();
    let v = road.air_variants[0];
    assert!(store.block_mesh("Stadium", &road.name, false, v.variant, v.sub_variant).unwrap().is_some());

    for vehicle in store.vehicles().unwrap().iter() {
        assert!(store.vehicle_mesh(&vehicle.name).unwrap().is_some(), "no mesh for {}", vehicle.name);
    }

    // Count the textures materials name that the export has; the web viewer skips the same missing ones
    let materials = store.materials().unwrap();
    let (mut found, mut missing) = (0, 0);
    for name in materials.keys() {
        if let MaterialSpec::Textured { textures, .. } = resolve_material(name, &materials, None) {
            for path in [textures.diffuse, textures.normal, textures.specular, textures.blend2, textures.blend_intensity, textures.blend3].into_iter().flatten() {
                if store.texture(&path).unwrap().is_some() {
                    found += 1;
                } else {
                    missing += 1;
                }
            }
        }
    }
    eprintln!("material textures: {found} found, {missing} missing");
    assert!(found > 0);
}
