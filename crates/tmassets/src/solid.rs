// ABOUTME: Decodes the gbx-tools-3d mesh format (api/mesh/{id}) into a node tree, and flattens it into drawable parts.
// ABOUTME: Port of solidDecoder.ts and collectMeshParts in solidToThree.ts, format versions 0 to 2.

use crate::{AssetError, Result};
use glam::{Mat3, Mat4, Vec3};
use std::io::Read;

const MAGIC: [u8; 6] = [0xD4, 0x54, 0x35, 0x84, 0x03, 0xCD];
const MAX_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct Solid {
    pub version: u32,
    pub lod: Option<u8>,
    pub file_path: Option<String>,
    pub root: SolidNode,
}

#[derive(Debug, Clone, Default)]
pub struct SolidNode {
    pub name: String,
    /// Row-major 3x3 rotation: xx, xy, xz, yx, yy, yz, zx, zy, zz
    pub rotation: Option<[f32; 9]>,
    pub position: Option<[f32; 3]>,
    pub visual: Option<Visual>,
    pub lods: Vec<(f32, SolidNode)>,
    pub children: Vec<SolidNode>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Visual {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    /// One list per UV set
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub indices: Vec<u32>,
    pub material_name: String,
}

/// One visual of a mesh with its transform within the whole mesh
#[derive(Debug, Clone)]
pub struct MeshPart {
    pub visual: Visual,
    pub matrix: Mat4,
}

fn format_err<T>(message: impl Into<String>) -> Result<T> {
    Err(AssetError::Format(message.into()))
}

/// Little-endian reader with .NET BinaryReader's 7-bit encoded lengths
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.bytes.len() - self.pos {
            return format_err(format!("mesh data ends early: wanted {n} bytes at {}", self.pos));
        }
        self.pos += n;
        Ok(&self.bytes[self.pos - n..self.pos])
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    fn f32(&mut self) -> Result<f32> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn var_u32(&mut self) -> Result<u32> {
        let mut result = 0u32;
        for shift in (0..35).step_by(7) {
            let b = self.u8()?;
            result |= ((b & 0x7F) as u32) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
        }
        format_err("7-bit encoded int is too long")
    }

    fn count(&mut self) -> Result<usize> {
        Ok(self.var_u32()? as usize)
    }

    fn string(&mut self) -> Result<String> {
        let n = self.count()?;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn uint(&mut self, size: u8) -> Result<u32> {
        let b = self.take(size as usize)?;
        Ok(match size {
            1 => b[0] as u32,
            2 => u16::from_le_bytes([b[0], b[1]]) as u32,
            4 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => return format_err(format!("unsupported index size {size}")),
        })
    }
}

pub fn decode_solid(data: &[u8]) -> Result<Solid> {
    let mut header = Reader { bytes: data, pos: 0 };
    if header.take(MAGIC.len())? != MAGIC {
        return format_err("not a gbx-tools-3d mesh (bad magic)");
    }
    let version = header.var_u32()?;
    if version > MAX_VERSION {
        return format_err(format!("unsupported mesh format version {version}"));
    }
    let lod = header.u8()?;

    let mut body = Vec::new();
    flate2::read::DeflateDecoder::new(&data[header.pos..])
        .read_to_end(&mut body)
        .map_err(|e| AssetError::Format(format!("mesh data doesn't inflate: {e}")))?;
    let mut r = Reader { bytes: &body, pos: 0 };

    if r.bool()? {
        r.take(8)?; // file write time
    }
    let file_path = if version >= 1 { Some(r.string()?) } else { None };
    let root = read_node(&mut r, version)?;
    if r.pos != body.len() {
        return format_err(format!("{} bytes left after the mesh tree; the format was misread", body.len() - r.pos));
    }
    Ok(Solid { version, lod: (lod != 255).then_some(lod), file_path, root })
}

fn read_node(r: &mut Reader, version: u32) -> Result<SolidNode> {
    let children_count = r.count()?;
    let rotation = if r.bool()? {
        let mut m = [0.0; 9];
        for v in &mut m {
            *v = r.f32()?;
        }
        Some(m)
    } else {
        None
    };
    let position = if r.bool()? { Some(r.vec3()?) } else { None };
    let visual = read_visual(r)?;

    let lod_count = r.count()?;
    let mut lods = Vec::with_capacity(lod_count);
    for _ in 0..lod_count {
        let distance = r.f32()?;
        lods.push((distance, read_node(r, version)?));
    }

    skip_surface(r)?;
    if version >= 2 {
        skip_light(r)?;
    }
    let name = r.string()?;
    let children = (0..children_count).map(|_| read_node(r, version)).collect::<Result<_>>()?;
    Ok(SolidNode { name, rotation, position, visual, lods, children })
}

fn read_visual(r: &mut Reader) -> Result<Option<Visual>> {
    if !r.bool()? {
        return Ok(None);
    }
    let has_normals = r.bool()?;
    let vertex_count = r.count()?;
    let uv_set_count = r.count()?;
    let mut uv_sets = Vec::with_capacity(uv_set_count);
    for _ in 0..uv_set_count {
        uv_sets.push((0..vertex_count).map(|_| Ok([r.f32()?, r.f32()?])).collect::<Result<Vec<_>>>()?);
    }
    let positions = (0..vertex_count).map(|_| r.vec3()).collect::<Result<Vec<_>>>()?;
    let normals = if has_normals { Some((0..vertex_count).map(|_| r.vec3()).collect::<Result<Vec<_>>>()?) } else { None };

    let index_count = r.count()?;
    let size = r.u8()?;
    let indices = (0..index_count).map(|_| r.uint(size)).collect::<Result<Vec<_>>>()?;

    let material_name = r.string()?;
    if r.bool()? {
        r.bool()?; // casts shadow
    }
    Ok(Some(Visual { positions, normals, uv_sets, indices, material_name }))
}

/// Collision surfaces aren't drawn; they are read only to step over them
fn skip_surface(r: &mut Reader) -> Result<()> {
    if !r.bool()? {
        return Ok(());
    }
    match r.var_u32()? {
        0 => r.take(4).map(|_| ()),
        1 => r.take(12).map(|_| ()),
        2 => {
            let vertices = r.count()?;
            r.take(vertices * 12)?;
            let triangles = r.count()?;
            let size = r.u8()? as usize;
            r.take(triangles * (1 + size * 3)).map(|_| ())
        }
        t => format_err(format!("unknown surface type {t}")),
    }
}

/// Lights are left out for now: the renderer uses the mood's sun and ambient
fn skip_light(r: &mut Reader) -> Result<()> {
    if !r.bool()? {
        return Ok(());
    }
    r.take(1 + 12 + 4)?; // night only, color, intensity
    match r.var_u32()? {
        0 => Ok(()),
        1 => r.take(4).map(|_| ()),
        2 => r.take(12).map(|_| ()),
        k => format_err(format!("unknown light type {k}")),
    }
}

/// Rotation then translation, as a three.js object with both set
fn local_matrix(node: &SolidNode) -> Mat4 {
    let rotation = node.rotation.map_or(Mat4::IDENTITY, |m| {
        // Rows of the stored matrix are the columns of its transpose
        Mat4::from_mat3(Mat3::from_cols_array(&m).transpose())
    });
    match node.position {
        Some(p) => Mat4::from_translation(Vec3::from(p)) * rotation,
        None => rotation,
    }
}

/// Every visual in the tree with its combined transform; only the most detailed LOD level is kept
pub fn mesh_parts(root: &SolidNode) -> Vec<MeshPart> {
    fn walk(node: &SolidNode, parent: Mat4, parts: &mut Vec<MeshPart>) {
        let matrix = parent * local_matrix(node);
        if let Some(visual) = &node.visual {
            parts.push(MeshPart { visual: visual.clone(), matrix });
        }
        if let Some((_, lod)) = node.lods.first() {
            walk(lod, matrix, parts);
        }
        for child in &node.children {
            walk(child, matrix, parts);
        }
    }
    let mut parts = Vec::new();
    walk(root, Mat4::IDENTITY, &mut parts);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn var(n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut n = n as u32;
        loop {
            let b = (n & 0x7F) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }

    fn string(s: &str) -> Vec<u8> {
        [var(s.len()), s.as_bytes().to_vec()].concat()
    }

    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// A node with one triangle, translated, with one child that has no visual
    fn sample_body() -> Vec<u8> {
        let visual = [
            vec![1, 0],                  // has visual, no normals
            var(3),                      // vertices
            var(1),                      // one UV set
            floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            var(3),
            vec![2],
            vec![0, 0, 1, 0, 2, 0],
            string("Stadium/Media/Material/StadiumRoad"),
            vec![1, 1], // casts shadow
        ]
        .concat();
        let child = [var(0), vec![0, 0, 0], var(0), vec![0, 0], string("child")].concat();
        [
            vec![0],                 // no write time
            string("Stadium/Solid/Test"),
            var(1),                  // one child
            vec![0],                 // no rotation
            vec![1],
            floats(&[32.0, 8.0, 0.0]),
            visual,
            var(0),                  // no LODs
            vec![1], var(0), floats(&[2.0]), // sphere surface
            vec![0],                 // no light
            string("root"),
            child,
        ]
        .concat()
    }

    fn encode(body: &[u8]) -> Vec<u8> {
        let mut deflate = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        deflate.write_all(body).unwrap();
        [MAGIC.to_vec(), var(2), vec![255], deflate.finish().unwrap()].concat()
    }

    #[test]
    fn decodes_a_mesh_tree() {
        let solid = decode_solid(&encode(&sample_body())).unwrap();
        assert_eq!((solid.version, solid.lod, solid.file_path.as_deref()), (2, None, Some("Stadium/Solid/Test")));
        assert_eq!(solid.root.name, "root");
        assert_eq!(solid.root.children[0].name, "child");
        let visual = solid.root.visual.as_ref().unwrap();
        assert_eq!(visual.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(visual.uv_sets[0][2], [0.0, 1.0]);
        assert_eq!(visual.indices, [0, 1, 2]);
        assert_eq!(visual.material_name, "Stadium/Media/Material/StadiumRoad");
    }

    #[test]
    fn rejects_bad_magic_and_trailing_bytes() {
        assert!(decode_solid(&[0; 16]).is_err());
        let mut body = sample_body();
        body.push(0);
        assert!(decode_solid(&encode(&body)).is_err());
    }

    #[test]
    fn parts_carry_their_node_transforms() {
        let solid = decode_solid(&encode(&sample_body())).unwrap();
        let parts = mesh_parts(&solid.root);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].matrix.transform_point3(Vec3::new(1.0, 0.0, 0.0)), Vec3::new(33.0, 8.0, 0.0));
    }

    #[test]
    fn stored_rotations_are_row_major() {
        // A quarter turn about Y stored row-major: x goes to -z
        let node = SolidNode { rotation: Some([0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0]), ..Default::default() };
        let p = local_matrix(&node).transform_point3(Vec3::X);
        assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
    }
}
