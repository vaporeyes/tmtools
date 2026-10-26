// ABOUTME: Chooses how to draw a mesh material from the export's material data, as GbxTools3D's Material.cs does.
// ABOUTME: Pure port of materials.ts: a plain spec of textures and shader hints for the renderer to turn into a pipeline.

use serde::Deserialize;
use std::collections::HashMap;

/// One entry of api/materials/{game}; omitted fields are absent or false
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialRecord {
    #[serde(default)]
    pub is_shader: bool,
    pub shader: Option<String>,
    #[serde(default)]
    pub textures: HashMap<String, String>,
    #[serde(default)]
    pub modifiers: HashMap<String, MaterialRecord>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShaderProperties {
    pub double_sided: bool,
    /// Texture coordinates come from world X and Z instead of the mesh (terrain)
    pub world_uv: bool,
    pub transparent: bool,
    /// Unlit, e.g. skies
    pub basic: bool,
    pub specular_alpha: bool,
    /// Fixed opacity in hundredths; 0 means not set
    pub opacity_percent: u8,
    pub add: bool,
    pub night_only: bool,
    pub invisible: bool,
    pub water: bool,
    pub no_wrap: bool,
    pub dds_flip_y: bool,
}

impl ShaderProperties {
    pub fn opacity(&self) -> Option<f32> {
        (self.opacity_percent != 0).then(|| self.opacity_percent as f32 / 100.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextureSlots {
    pub diffuse: Option<String>,
    pub normal: Option<String>,
    pub specular: Option<String>,
    pub blend2: Option<String>,
    pub blend_intensity: Option<String>,
    pub blend3: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialSpec {
    Textured { shader: Option<String>, textures: TextureSlots, properties: ShaderProperties },
    Glass,
    Invisible,
    /// No usable entry; the renderer picks a stand-in color from the name
    Unknown,
}

/// Shader-specific drawing hints, copied from Material.cs
fn shader_properties(shader: &str) -> Option<ShaderProperties> {
    let p = |f: &dyn Fn(&mut ShaderProperties)| {
        let mut p = ShaderProperties::default();
        f(&mut p);
        p
    };
    Some(match shader {
        "Techno/Media/Material/PDiff PDiff PA PX2"
        | "Techno2/Media/Material/PDiff PDiff PA PX2 Grass2"
        | "Techno2/Media/Material/PDiff PDiff PA TOcc PX2 Grass"
        | "Techno2/Media/Material/PDiff PDiff PA TOcc PX2 Grass NoLightV"
        | "Techno/Media/Material/PDiff Fresnel PX2"
        | "Techno/Media/Material/PDisp PDiff PX2"
        | "Techno2/Media/Material/SoilGen21"
        | "Techno3/Media/Material/Tech3 Block PDiff_Spec_Norm"
        | "Techno3/Media/Material/Tech3 Block PDiff_Spec_Norm GrassX2"
        | "Techno3/Media/Material/Tech3 Warp PyaPxzDiff"
        | "Techno3/Media/Material/Tech3 Warp_PyaDiff_To_PDiffPGrassX2" => p(&|p| p.world_uv = true),
        "Techno2/Media/Material/TDiff_Spec_Nrm TOcc CSpecSoft"
        | "Techno/Media/Material/TDiffG PX2 CSpec FCOut Trans"
        | "Techno/Media/Material/TDiffG PX2 CSpecL Trans"
        | "Techno2/Media/Material/TDiff_Spec_Nrm TOcc CSpecSoft NoLightV"
        | "Island/Media/Material/ModelAlpha1SidedLight"
        | "Techno2/Media/Material/TDiff_Spec_Nrm TOcc CSpecSoft Trans"
        | "Techno3/Media/Material/Tech3 Block TDiffA_Spec_Norm"
        | "Techno3/Media/Material/Tech3_Block_TDiffABlend_SpecNorm_CubeOut" => p(&|p| p.transparent = true),
        "Techno/Media/Material/TDiff PX2 Trans"
        | "Techno/Media/Material/TDiff PX2 Trans 2Sided"
        | "Techno/Media/Material/TDiff PX2 Trans NormY PC3only"
        | "Island/Media/Material/ModelAlpha2SidedNoLight" => p(&|p| {
            p.transparent = true;
            p.double_sided = true;
        }),
        "Techno/Media/Material/Sky" | "Island/Media/Material/IslandSky" | "Sky/Media/Material/SkyDay" => p(&|p| p.basic = true),
        "Techno/Media/Material/TDiffG PX2 CSpecL_Pixel" => p(&|p| p.specular_alpha = true),
        "Vehicles/Media/Material/SportCarGlass" | ":Glass" => p(&|p| {
            p.transparent = true;
            p.opacity_percent = 89;
        }),
        "Techno/Media/Material/TAdd" | "Techno/Media/Material/TAdd ZBias" | "Alpine/Media/Material/AlpineSignsSelfIllum" | "Techno2/Media/Material/TSelfI Add" => {
            p(&|p| {
                p.transparent = true;
                p.add = true;
            })
        }
        "Techno/Media/Material/TAdd Night" | "Techno/Media/Material/TAdd Night ZBias" | "Island/Media/Material/ModelLightVolume" => p(&|p| {
            p.transparent = true;
            p.add = true;
            p.night_only = true;
        }),
        "Techno2/Media/Material/VDep Fence" | "Techno/Media/Material/ShadowSkirt" | ":FakeShad" | "Techno3/Media/Material/Sky/Tech3 Sky" => {
            p(&|p| p.invisible = true)
        }
        "Techno/Media/Material/Sea" | "Techno/Media/Material/SeaMultiY" | "Techno3/Media/Material/Tech3 Sea" => p(&|p| p.water = true),
        "Island/Media/Material/IslandBeachFoam" => p(&|p| {
            p.add = true;
            p.no_wrap = true;
        }),
        "Island/Media/Material/IslandWindowsMip" => p(&|p| {
            p.transparent = true;
            p.opacity_percent = 70;
        }),
        ":DDSFlipY" => p(&|p| p.dds_flip_y = true),
        _ => return None,
    })
}

// First present slot wins, in the same order as Material.cs
const DIFFUSE_SLOTS: [&str; 13] =
    ["Diffuse", "Blend1", "Panorama", "Advert", "Glow", "Soil", "Grass", "Foam 1", "GDiffuse", "PxzDiffuse", "PyDiffuse", "BaseColor", "PxzBaseColor"];
const SPECULAR_SLOTS: [&str; 2] = ["Specular", "PxzSpecular"];
const BLEND3_SLOTS: [&str; 4] = ["Blend3", "Borders", "SoilFix", "DiffuseBlendA"];

pub fn resolve_material(name: &str, materials: &HashMap<String, MaterialRecord>, terrain_modifier: Option<&str>) -> MaterialSpec {
    let Some(mut record) = materials.get(name) else { return MaterialSpec::Unknown };

    if let Some(modifier) = terrain_modifier {
        // Block data and material data disagree on casing for some modifiers ("fabric" vs "Fabric")
        if let Some((_, swapped)) = record.modifiers.iter().find(|(k, _)| k.eq_ignore_ascii_case(modifier)) {
            record = swapped;
        }
    }

    let textures = &record.textures;
    // Shader-only entries (collision helpers, car stand-ins) draw nothing, apart from glass
    if record.is_shader && textures.is_empty() {
        return if name == ":Glass" { MaterialSpec::Glass } else { MaterialSpec::Invisible };
    }
    if textures.is_empty() {
        return MaterialSpec::Unknown;
    }

    let properties = record
        .shader
        .as_deref()
        .and_then(|s| shader_properties(&s.replace('\\', "/")))
        .or_else(|| if record.is_shader { shader_properties(&name.replace('\\', "/")) } else { None })
        .unwrap_or_default();
    let first = |slots: &[&str]| slots.iter().find_map(|s| textures.get(*s).cloned());

    MaterialSpec::Textured {
        shader: record.shader.clone(),
        textures: TextureSlots {
            diffuse: first(&DIFFUSE_SLOTS),
            normal: textures.get("Normal").cloned(),
            specular: first(&SPECULAR_SLOTS),
            blend2: textures.get("Blend2").cloned(),
            blend_intensity: textures.get("BlendI").cloned(),
            blend3: first(&BLEND3_SLOTS),
        },
        properties,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Records mirror real api/materials/TMF entries, trimmed to the fields that matter (from materials.test.ts)
    fn materials() -> HashMap<String, MaterialRecord> {
        serde_json::from_str(
            r#"{
            "Stadium/Media/Material/StadiumGrass": {
                "shader": "Techno2/Media/Material/PDiff PDiff PA PX2 Grass2",
                "textures": { "Blend1": "Stadium/Media/Texture/StadiumGrass1", "Blend2": "Stadium/Media/Texture/StadiumGrass2", "BlendI": "Stadium/Media/Texture/StadiumGrassMask" },
                "modifiers": { "TerrainModifierDirt": { "shader": "Techno2/Media/Material/SoilGen21", "textures": { "Soil": "Stadium/Media/Texture/StadiumDirt" } } }
            },
            "Stadium/Media/Material/StadiumRoad": {
                "shader": "Techno2/Media/Material/TDiff_Spec_Nrm TOcc CSpecSoft",
                "textures": { "Diffuse": "Stadium/Media/Texture/StadiumRoad", "Normal": "Stadium/Media/Texture/StadiumRoadN", "Specular": "Stadium/Media/Texture/StadiumRoadS" }
            },
            "Stadium/Media/Material/StadiumBorder": {
                "shader": "Techno/Media/Material/Custom",
                "textures": { "Diffuse": "Stadium/Media/Texture/Border", "DiffuseBlendA": "Stadium/Media/Texture/BorderDirt" }
            },
            ":Glass": { "isShader": true },
            ":FakeShad": { "isShader": true },
            "Stadium/Media/Material/Untextured": { "shader": "Techno/Media/Material/Anything" }
        }"#,
        )
        .unwrap()
    }

    fn textured(spec: MaterialSpec) -> (TextureSlots, ShaderProperties) {
        match spec {
            MaterialSpec::Textured { textures, properties, .. } => (textures, properties),
            other => panic!("expected a textured material, got {other:?}"),
        }
    }

    #[test]
    fn uses_the_standard_textures_and_the_shaders_properties() {
        let (t, p) = textured(resolve_material("Stadium/Media/Material/StadiumRoad", &materials(), None));
        assert_eq!(t.diffuse.as_deref(), Some("Stadium/Media/Texture/StadiumRoad"));
        assert_eq!(t.normal.as_deref(), Some("Stadium/Media/Texture/StadiumRoadN"));
        assert_eq!(t.specular.as_deref(), Some("Stadium/Media/Texture/StadiumRoadS"));
        assert!(p.transparent && !p.world_uv);
    }

    #[test]
    fn grass_takes_blend1_as_diffuse_and_world_uvs() {
        let (t, p) = textured(resolve_material("Stadium/Media/Material/StadiumGrass", &materials(), None));
        assert_eq!(t.diffuse.as_deref(), Some("Stadium/Media/Texture/StadiumGrass1"));
        assert_eq!(t.blend2.as_deref(), Some("Stadium/Media/Texture/StadiumGrass2"));
        assert_eq!(t.blend_intensity.as_deref(), Some("Stadium/Media/Texture/StadiumGrassMask"));
        assert!(p.world_uv);
    }

    #[test]
    fn terrain_modifiers_swap_the_material_regardless_of_case() {
        for modifier in ["TerrainModifierDirt", "terrainmodifierdirt"] {
            let (t, _) = textured(resolve_material("Stadium/Media/Material/StadiumGrass", &materials(), Some(modifier)));
            assert_eq!(t.diffuse.as_deref(), Some("Stadium/Media/Texture/StadiumDirt"));
        }
        let (t, _) = textured(resolve_material("Stadium/Media/Material/StadiumRoad", &materials(), Some("Fabric")));
        assert_eq!(t.diffuse.as_deref(), Some("Stadium/Media/Texture/StadiumRoad"));
    }

    #[test]
    fn picks_up_the_extra_blend_layer() {
        let (t, _) = textured(resolve_material("Stadium/Media/Material/StadiumBorder", &materials(), None));
        assert_eq!(t.blend3.as_deref(), Some("Stadium/Media/Texture/BorderDirt"));
    }

    #[test]
    fn untextured_shaders_are_invisible_except_glass_and_unknowns_are_reported() {
        let m = materials();
        assert_eq!(resolve_material(":FakeShad", &m, None), MaterialSpec::Invisible);
        assert_eq!(resolve_material(":Glass", &m, None), MaterialSpec::Glass);
        assert_eq!(resolve_material("Nope", &m, None), MaterialSpec::Unknown);
        assert_eq!(resolve_material("Stadium/Media/Material/Untextured", &m, None), MaterialSpec::Unknown);
    }
}
