// ABOUTME: A vehicle mesh rigged for animation: wheels spin and steer, front guards steer, and wheels, guards, and rear
// ABOUTME: hubs follow the suspension, driven by the ghost's samples. Port of ghostAnimation.ts (GhostControls in gbx-tools-3d).

use crate::scene::{geometry, surface, DrawPart};
use glam::{EulerRot, Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::sync::Arc;
use tmassets::material::MaterialRecord;
use tmassets::solid::{local_matrix, SolidNode};
use tmreplay::Sample;

// Sample columns are ordered FL, FR, RR, RL
const FL: usize = 0;
const FR: usize = 1;
const RR: usize = 2;
const RL: usize = 3;

// Vehicle meshes repeat parts per LOD and shading variant under these name prefixes
const PREFIXES: [&str; 5] = ["1", "d", "p", "s", "w"];

#[derive(Debug, Clone, Copy, PartialEq)]
struct JointKind {
    corner: usize,
    /// Wheels spin around X and set their steer angle around Y (0 at the rear); guards only steer at the front
    wheel: bool,
    steers: bool,
}

fn joint_kind(name: &str) -> Option<JointKind> {
    let base = PREFIXES.iter().find_map(|p| name.strip_prefix(p))?;
    let (corner, wheel, steers) = match base {
        "FLWheel" => (FL, true, true),
        "FRWheel" => (FR, true, true),
        "RLWheel" => (RL, true, false),
        "RRWheel" => (RR, true, false),
        "FLGuard" => (FL, false, true),
        "FRGuard" => (FR, false, true),
        "RLHub" => (RL, false, false),
        "RRHub" => (RR, false, false),
        _ => return None,
    };
    Some(JointKind { corner, wheel, steers })
}

/// An animated node: what moves it, and its rest pose (position, and rotation as YXZ Euler angles like three.js)
#[derive(Debug, Clone, PartialEq)]
struct Joint {
    kind: JointKind,
    rest_position: Vec3,
    /// Rotation about Y, then X, then Z
    rest_euler: (f32, f32, f32),
}

impl Joint {
    fn local(&self, state: &WheelState) -> Mat4 {
        let (rest_y, rest_x, rest_z) = self.rest_euler;
        let k = self.kind;
        let x = if k.wheel { state.spin[k.corner] } else { rest_x };
        // Rear wheels get a steer track of zeros; rear hubs have none and keep their rest angle
        let y = if k.steers { state.steer } else if k.wheel { 0.0 } else { rest_y };
        // Suspension length is a downward offset from the part's rest height
        let position = Vec3::new(self.rest_position.x, self.rest_position.y - state.dampen[k.corner], self.rest_position.z);
        Mat4::from_rotation_translation(Quat::from_euler(EulerRot::YXZ, y, x, rest_z), position)
    }

    fn rest(&self) -> Mat4 {
        let (y, x, z) = self.rest_euler;
        Mat4::from_rotation_translation(Quat::from_euler(EulerRot::YXZ, y, x, z), self.rest_position)
    }
}

/// Where a part hangs: fixed in the car, or under an animated node with the transforms around it
#[derive(Debug, Clone, PartialEq)]
enum Link {
    Fixed(Mat4),
    Joint { index: usize, before: Mat4, after: Mat4 },
}

/// The wheels' state at a moment: spin and suspension per wheel (FL, FR, RR, RL) and the front steer angle
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WheelState {
    pub spin: [f32; 4],
    pub steer: f32,
    pub dampen: [f32; 4],
}

/// The wheel state at time_ms, interpolated between samples as three.js keyframe tracks do; held at the ends
pub fn wheel_state(samples: &[Sample], time_ms: f32) -> Option<WheelState> {
    let last = samples.len().checked_sub(1)?;
    let i = samples.partition_point(|s| (s.time_ms as f32) <= time_ms);
    let (a, b, t) = if i == 0 {
        (0, 0, 0.0)
    } else if i > last {
        (last, last, 0.0)
    } else {
        let span = (samples[i].time_ms - samples[i - 1].time_ms) as f32;
        (i - 1, i, if span > 0.0 { (time_ms - samples[i - 1].time_ms as f32) / span } else { 0.0 })
    };
    let (sa, sb) = (&samples[a], &samples[b]);
    let mix = |x: f32, y: f32| x + (y - x) * t;
    Some(WheelState {
        spin: std::array::from_fn(|w| mix(sa.wheel_rotation[w], sb.wheel_rotation[w])),
        steer: mix(sa.steer_front, sb.steer_front),
        dampen: std::array::from_fn(|w| mix(sa.dampen_len[w], sb.dampen_len[w])),
    })
}

/// A vehicle's drawable parts with the rig that poses them
#[derive(Debug, Clone)]
pub struct VehicleModel {
    /// Parts in car space at rest; instances hold the rest transform
    pub parts: Vec<DrawPart>,
    links: Vec<Link>,
    joints: Vec<Joint>,
}

impl VehicleModel {
    pub fn from_solid(root: &SolidNode, materials: &HashMap<String, MaterialRecord>) -> Self {
        let mut model = Self { parts: Vec::new(), links: Vec::new(), joints: Vec::new() };
        let mut names = Vec::new();
        model.walk(root, Link::Fixed(Mat4::IDENTITY), &mut names, materials);
        model
    }

    fn walk(&mut self, node: &SolidNode, parent: Link, names: &mut Vec<String>, materials: &HashMap<String, MaterialRecord>) {
        let link = match parent {
            // Only the first node of each name animates, as getObjectByName finds just one
            Link::Fixed(world) => match joint_kind(&node.name).filter(|_| !names.contains(&node.name)) {
                Some(kind) => {
                    names.push(node.name.clone());
                    let rest = local_matrix(node);
                    let (_, rotation, position) = rest.to_scale_rotation_translation();
                    self.joints.push(Joint { kind, rest_position: position, rest_euler: rotation.to_euler(EulerRot::YXZ) });
                    Link::Joint { index: self.joints.len() - 1, before: world, after: Mat4::IDENTITY }
                }
                None => Link::Fixed(world * local_matrix(node)),
            },
            Link::Joint { index, before, after } => Link::Joint { index, before, after: after * local_matrix(node) },
        };

        if let Some(visual) = &node.visual {
            let rest = self.place(&link, None);
            self.parts.push(DrawPart {
                surface: surface(&visual.material_name, materials, None),
                material_name: visual.material_name.clone(),
                geometry: Arc::new(geometry(visual)),
                flip_winding: rest.determinant() < 0.0,
                instances: vec![rest],
            });
            self.links.push(link.clone());
        }
        // Only the most detailed LOD level is drawn
        if let Some((_, lod)) = node.lods.first() {
            self.walk(lod, link.clone(), names, materials);
        }
        for child in &node.children {
            self.walk(child, link.clone(), names, materials);
        }
    }

    fn place(&self, link: &Link, state: Option<&WheelState>) -> Mat4 {
        match link {
            Link::Fixed(m) => *m,
            Link::Joint { index, before, after } => {
                let joint = &self.joints[*index];
                *before * state.map_or_else(|| joint.rest(), |s| joint.local(s)) * *after
            }
        }
    }

    /// Each part's transform in car space for this wheel state, in the order of `parts`
    pub fn part_matrices(&self, state: Option<&WheelState>) -> Vec<Mat4> {
        self.links.iter().map(|link| self.place(link, state)).collect()
    }

    /// How many nodes the rig animates
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmassets::solid::Visual;

    fn node(name: &str, position: Option<[f32; 3]>, visual: bool, children: Vec<SolidNode>) -> SolidNode {
        let visual = visual.then(|| Visual { positions: vec![[0.0; 3]; 3], indices: vec![0, 1, 2], material_name: "m".into(), ..Default::default() });
        SolidNode { name: name.into(), position, visual, children, ..Default::default() }
    }

    /// A car root with a body and a front-left wheel whose mesh is a child of the wheel node
    fn car() -> SolidNode {
        node("1", None, false, vec![node("1Body", None, true, vec![]), node("1FLWheel", Some([0.7, 0.35, 1.8]), false, vec![node("tyre", None, true, vec![])])])
    }

    #[test]
    fn finds_the_joints_and_keeps_fixed_parts_still() {
        let model = VehicleModel::from_solid(&car(), &HashMap::new());
        assert_eq!(model.joint_count(), 1);
        assert_eq!(model.parts.len(), 2);
        let state = WheelState { spin: [1.0; 4], steer: 0.3, dampen: [0.1; 4] };
        let posed = model.part_matrices(Some(&state));
        assert_eq!(posed[0], Mat4::IDENTITY);
        // At rest the tyre sits at the wheel node
        let rest = model.part_matrices(None);
        assert!((rest[1].transform_point3(Vec3::ZERO) - Vec3::new(0.7, 0.35, 1.8)).length() < 1e-6);
    }

    #[test]
    fn wheels_drop_with_the_suspension_and_spin_then_steer() {
        let model = VehicleModel::from_solid(&car(), &HashMap::new());
        let state = WheelState { spin: [std::f32::consts::FRAC_PI_2, 0.0, 0.0, 0.0], steer: std::f32::consts::FRAC_PI_2, dampen: [0.1, 0.0, 0.0, 0.0] };
        let m = model.part_matrices(Some(&state))[1];
        assert!((m.transform_point3(Vec3::ZERO) - Vec3::new(0.7, 0.25, 1.8)).length() < 1e-6);
        // YXZ order: spinning a quarter turn about X takes +Y to +Z, then steering a quarter turn about Y takes +Z to +X
        let up = m.transform_vector3(Vec3::Y);
        assert!((up - Vec3::X).length() < 1e-5, "{up}");
    }

    #[test]
    fn rear_hubs_keep_their_rest_angle_and_only_the_first_named_node_animates() {
        assert_eq!(joint_kind("1RLHub"), Some(JointKind { corner: RL, wheel: false, steers: false }));
        assert_eq!(joint_kind("sFRGuard"), Some(JointKind { corner: FR, wheel: false, steers: true }));
        assert_eq!(joint_kind("1FLHub"), None);
        let twice = node("1", None, false, vec![node("1FLWheel", None, true, vec![]), node("1FLWheel", None, true, vec![])]);
        assert_eq!(VehicleModel::from_solid(&twice, &HashMap::new()).joint_count(), 1);
    }

    #[test]
    fn wheel_state_interpolates_between_samples() {
        let sample = |t: i32, v: f32| Sample { time_ms: t, wheel_rotation: [v; 4], steer_front: v, dampen_len: [v; 4], ..Default::default() };
        let samples = [sample(0, 0.0), sample(100, 1.0)];
        let s = wheel_state(&samples, 25.0).unwrap();
        assert_eq!((s.spin[2], s.steer, s.dampen[3]), (0.25, 0.25, 0.25));
        assert_eq!(wheel_state(&samples, 500.0).unwrap().steer, 1.0);
        assert!(wheel_state(&[], 0.0).is_none());
    }
}
