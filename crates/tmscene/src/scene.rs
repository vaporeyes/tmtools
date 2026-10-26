// ABOUTME: Assembles drawable parts from an asset export: placed block variants, the decoration scene, and vehicles,
// ABOUTME: each with flattened geometry, a resolved material, and its instance transforms in world space.

use crate::mood::{mood_for, Mood};
use crate::placement::{plan_blocks, GridOptions};
use crate::vehicle::VehicleModel;
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;
use tmassets::material::{resolve_material, MaterialRecord, MaterialSpec};
use tmassets::solid::{mesh_parts, Solid, Visual};
use tmassets::AssetStore;
use tmreplay::ReplayMap;

/// Triangle geometry ready for upload: one normal and one UV per vertex
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// How a part is drawn
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// Textured from the export; see MaterialSpec::Textured
    Textured(MaterialSpec),
    /// No usable material data: a stand-in color chosen from the material's name
    Color { rgb: [u8; 3], opacity: f32, emissive: bool },
    Glass,
    Invisible,
}

#[derive(Debug, Clone)]
pub struct DrawPart {
    pub material_name: String,
    pub surface: Surface,
    pub geometry: Arc<Geometry>,
    /// World transform of each copy
    pub instances: Vec<Mat4>,
    /// The transform mirrors the mesh, so its triangles' winding must be reversed to keep front faces in front.
    /// Set as three.js does for the web viewer: for single meshes (decoration and cars), not for instanced blocks.
    pub flip_winding: bool,
}

pub struct MapScene {
    pub parts: Vec<DrawPart>,
    pub mood: Mood,
    /// Blocks or variants the export has no data for
    pub missing: Vec<String>,
    /// Meshes that couldn't be read
    pub failed: Vec<String>,
}

/// Builds geometry from a visual, computing smooth normals when the mesh has none
pub fn geometry(visual: &Visual) -> Geometry {
    let normals = visual.normals.clone().unwrap_or_else(|| vertex_normals(&visual.positions, &visual.indices));
    let uvs = visual.uv_sets.first().cloned().unwrap_or_else(|| vec![[0.0; 2]; visual.positions.len()]);
    Geometry { positions: visual.positions.clone(), normals, uvs, indices: visual.indices.clone() }
}

/// Area-weighted vertex normals, as three.js computeVertexNormals does for indexed geometry
fn vertex_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut sums = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
        let (pa, pb, pc) = (Vec3::from(positions[a]), Vec3::from(positions[b]), Vec3::from(positions[c]));
        let n = (pc - pb).cross(pa - pb);
        for i in [a, b, c] {
            sums[i] += n;
        }
    }
    sums.into_iter().map(|n| n.try_normalize().unwrap_or(Vec3::Y).to_array()).collect()
}

/// Stand-in colors by material name, first match wins (from mapMaterials.ts)
fn stand_in(material_name: &str) -> Surface {
    let name = material_name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    let color = |rgb: u32| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
    let (rgb, opacity, emissive) = if has(&["water", "pool"]) {
        (0x3f7fb5, 0.8, false)
    } else if has(&["glass", "window", "alpha", "grid", "fence", "net"]) {
        (0xc8d3dc, 0.45, false)
    } else if has(&["light", "neon", "lamp", "sign", "screen", "turbo", "boost"]) {
        (0xffe28a, 1.0, true)
    } else if has(&["grass", "lawn", "turf"]) {
        (0x6f9a4e, 1.0, false)
    } else if has(&["dirt", "mud", "sand"]) {
        (0x9b7a55, 1.0, false)
    } else if has(&["inflatable"]) {
        (0xd7d2c4, 1.0, false)
    } else if has(&["road", "asphalt", "tarmac", "track", "circuit", "tilt", "slope", "curve"]) {
        (0x4b5157, 1.0, false)
    } else if has(&["checkpoint", "start", "finish", "arrow"]) {
        (0xe6e9ec, 1.0, false)
    } else if has(&["platform", "concrete", "structure", "pillar", "tube", "sculpt", "metal", "support", "pylon", "stadium"]) {
        (0xaeb6bd, 1.0, false)
    } else {
        (0x9aa3ab, 1.0, false)
    };
    Surface::Color { rgb: color(rgb), opacity, emissive }
}

pub fn surface(material_name: &str, materials: &HashMap<String, MaterialRecord>, terrain_modifier: Option<&str>) -> Surface {
    match resolve_material(material_name, materials, terrain_modifier) {
        MaterialSpec::Unknown => stand_in(material_name),
        MaterialSpec::Glass => Surface::Glass,
        MaterialSpec::Invisible => Surface::Invisible,
        spec @ MaterialSpec::Textured { .. } => {
            if matches!(&spec, MaterialSpec::Textured { properties, .. } if properties.invisible) {
                Surface::Invisible
            } else {
                Surface::Textured(spec)
            }
        }
    }
}

/// The parts of one mesh, each placed at every transform in `at`; `single` marks a mesh drawn once (not instanced),
/// whose mirroring transforms flip its winding
fn solid_parts(solid: &Solid, at: &[Mat4], materials: &HashMap<String, MaterialRecord>, terrain_modifier: Option<&str>, single: bool) -> Vec<DrawPart> {
    mesh_parts(&solid.root)
        .into_iter()
        .map(|part| {
            let instances: Vec<Mat4> = at.iter().map(|m| *m * part.matrix).collect();
            DrawPart {
                surface: surface(&part.visual.material_name, materials, terrain_modifier),
                material_name: part.visual.material_name.clone(),
                geometry: Arc::new(geometry(&part.visual)),
                flip_winding: single && instances.first().is_some_and(|m| m.determinant() < 0.0),
                instances,
            }
        })
        .filter(|p| p.surface != Surface::Invisible)
        .collect()
}

/// The map's blocks and decoration as drawable parts, lit by the map's mood; None when the map has no readable blocks
pub fn build_map(store: &AssetStore, map: &ReplayMap) -> Result<Option<MapScene>, String> {
    let (Some(blocks), Some(collection_name), Some(size)) = (&map.blocks, map.collection.as_deref(), map.size) else { return Ok(None) };
    let err = |e: tmassets::AssetError| e.to_string();
    let collection = store.collection(collection_name).map_err(err)?.ok_or_else(|| format!("The {collection_name} environment isn't in the asset export"))?;
    let infos = store.block_infos(collection_name).map_err(err)?;
    let materials = store.materials().map_err(err)?;
    // The surroundings are optional; the track is still worth showing without them
    let decorations = store.decorations(collection_name).ok();
    let decoration = decorations.as_ref().and_then(|d| d.iter().find(|d| d.size == size).cloned());
    let base_height = decoration.as_ref().map_or(5, |d| d.base_height());

    let grid = GridOptions { square_size: collection.square_size, square_height: collection.square_height, base_height };
    let plan = plan_blocks(size, blocks, &infos, grid);

    let mut parts = Vec::new();
    let mut failed = Vec::new();
    for group in &plan.groups {
        let placements: Vec<Mat4> = group.placements.iter().map(|p| Mat4::from_translation(Vec3::new(p.x, p.y, p.z)) * Mat4::from_rotation_y(p.rotation_y)).collect();
        match store.block_mesh(collection_name, &group.name, group.is_ground, group.variant, group.sub_variant) {
            Ok(Some(solid)) => parts.extend(solid_parts(&solid, &placements, &materials, group.terrain_modifier.as_deref(), false)),
            Ok(None) => failed.push(format!("{} (no mesh)", group.name)),
            Err(e) => failed.push(format!("{}: {e}", group.name)),
        }
    }

    // Far-clip helpers are skipped as gbx-tools-3d does, and the sky dome too: the mood's sky replaces it
    for object in decoration.iter().flat_map(|d| d.scene.iter()) {
        let Some(solid_path) = object.solid.as_deref() else { continue };
        let lower = solid_path.to_ascii_lowercase();
        if lower.contains("farclip") || lower.contains("skydome") {
            continue;
        }
        match store.decoration_mesh(collection_name, solid_path) {
            Ok(Some(solid)) => parts.extend(solid_parts(&solid, &[object.matrix()], &materials, None, true)),
            Ok(None) => failed.push(format!("{solid_path} (no mesh)")),
            Err(e) => failed.push(format!("{solid_path}: {e}")),
        }
    }

    Ok(Some(MapScene { parts, mood: mood_for(map.decoration.as_deref(), Some(collection_name)), missing: plan.missing, failed }))
}

/// A vehicle's parts in the car's own space, rigged for wheel animation; None when the export has no mesh for it
pub fn vehicle_model(store: &AssetStore, vehicle: &str) -> Result<Option<VehicleModel>, String> {
    let materials = store.materials().map_err(|e| e.to_string())?;
    let Some(solid) = store.vehicle_mesh(vehicle).map_err(|e| e.to_string())? else { return Ok(None) };
    Ok(Some(VehicleModel::from_solid(&solid.root, &materials)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_area_weighted_normals() {
        // Two triangles of a unit square on the ground, wound counter-clockwise seen from above
        let positions = [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
        let normals = vertex_normals(&positions, &[0, 1, 2, 0, 2, 3]);
        assert!(normals.iter().all(|n| (Vec3::from(*n) - Vec3::Y).length() < 1e-6));
    }

    #[test]
    fn missing_uvs_become_zeros() {
        let visual = Visual { positions: vec![[0.0; 3]; 3], indices: vec![0, 1, 2], ..Default::default() };
        assert_eq!(geometry(&visual).uvs, vec![[0.0, 0.0]; 3]);
    }

    #[test]
    fn unknown_materials_get_stand_in_colors_by_name() {
        let none = HashMap::new();
        assert_eq!(surface("Stadium/Media/Material/StadiumRoadAsphalt", &none, None), Surface::Color { rgb: [0x4b, 0x51, 0x57], opacity: 1.0, emissive: false });
        assert!(matches!(surface("PoolWater", &none, None), Surface::Color { opacity, .. } if opacity < 1.0));
    }
}
