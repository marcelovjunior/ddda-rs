//! Authored normal-camera settings from `rCameraParamList` XFS.
//! Loader 0x007d01b0, reflected base settings 0x007b5250, collision
//! consumer 0x007e1400 (DDDA.exe 2017-12-13 Steam).

use crate::{Error, Result};
use mtxfs::{Object, Value, Xfs};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normal {
    pub distance: f32,
    pub height: f32,
    pub fov_degrees: f32,
    pub camera_target: u32,
    pub screen_adjust_type: u32,
    pub screen_adjust_flags: u32,
    pub screen_adjust_height: f32,
}

impl Normal {
    /// Normal gameplay branch of 0x007e1400 (2017-12-13 Steam).
    /// Actor positions are supplied by the caller selected for this view:
    /// world position first, model position second. Special camera targets,
    /// per-camera override parameters and the overlap query stay with callers.
    pub fn collision_anchor(
        &self,
        target: [f32; 3],
        actor: Option<([f32; 3], [f32; 3])>,
        original_pitch: f32,
        advancing: bool,
        actor_overlap: bool,
    ) -> Option<[f32; 3]> {
        if self.screen_adjust_flags & 1 == 0 {
            return None;
        }
        let mut anchor = match (self.screen_adjust_type, actor) {
            (1, Some((world, _))) => [
                world[0],
                if self.screen_adjust_flags & 4 != 0 {
                    target[1]
                } else {
                    world[1] + self.screen_adjust_height
                },
                world[2],
            ],
            (2, Some((_, model))) => model,
            _ => target,
        };
        if let Some((_, model)) = actor {
            if advancing && (actor_overlap || original_pitch < -0.8) {
                anchor = [model[0], model[1] + self.height, model[2]];
            }
        }
        Some(anchor)
    }
}

/// Slots preserve the authored array indices, including null and other
/// camera types. This reader does not select a gameplay camera policy.
#[derive(Debug, PartialEq)]
pub struct Parameters {
    pub normal: Vec<Option<Normal>>,
}

impl Parameters {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let xfs = mtxfs::parse(bytes).map_err(|e| Error(e.to_string()))?;
        Self::from_xfs(&xfs)
    }

    fn from_xfs(xfs: &Xfs) -> Result<Self> {
        if xfs.class_of(&xfs.root).name() != Some("rCameraParamList") {
            return Err(Error("expected rCameraParamList".into()));
        }
        let Some([Value::Object(Some(array))]) = xfs.get(&xfs.root, "mParamList") else {
            return Err(Error("missing camera parameter array".into()));
        };
        let slots = xfs
            .get(array, "mpArray")
            .ok_or_else(|| Error("missing camera parameter slots".into()))?;
        let mut normal = Vec::with_capacity(slots.len());
        for slot in slots {
            match slot {
                Value::Object(Some(object)) if xfs.class_of(object).name() == Some("cCamExParamNml") => {
                    normal.push(Some(Normal {
                        distance: scalar(xfs, object, "mDistance")?,
                        height: scalar(xfs, object, "mHeight")?,
                        fov_degrees: scalar(xfs, object, "mFov")?,
                        camera_target: integer(xfs, object, "mCameraTarget")?,
                        screen_adjust_type: integer(xfs, object, "mScrAdjustType")?,
                        screen_adjust_flags: integer(xfs, object, "mScrAdjustFlag")?,
                        screen_adjust_height: scalar(xfs, object, "mScrAdjustHeight")?,
                    }));
                }
                Value::Object(_) => normal.push(None),
                _ => return Err(Error("camera parameter slot is not an object".into())),
            }
        }
        Ok(Self { normal })
    }
}

fn scalar(xfs: &Xfs, object: &Object, name: &str) -> Result<f32> {
    match xfs.get(object, name) {
        Some([Value::F(values)]) if values.len() == 1 => Ok(values[0] as f32),
        _ => Err(Error(format!("missing scalar camera parameter {name}"))),
    }
}

fn integer(xfs: &Xfs, object: &Object, name: &str) -> Result<u32> {
    let value = match xfs.get(object, name) {
        Some([Value::U(v)]) => u32::try_from(*v).ok(),
        Some([Value::S(v)]) => u32::try_from(*v).ok(),
        _ => None,
    };
    value.ok_or_else(|| Error(format!("invalid integer camera parameter {name}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtxfs::{ClassDef, Property, Type};

    fn fixture() -> Xfs {
        let class = |name, fields: &[(&str, Type)]| ClassDef {
            hash: mtarc::type_hash(name),
            engine_value: 0,
            properties: fields
                .iter()
                .map(|(name, ty)| Property {
                    name: (*name).into(),
                    ty: *ty,
                    attr: 0,
                    size: 0,
                })
                .collect(),
        };
        let camera = Object {
            class: 2,
            number: 2,
            fields: vec![
                vec![Value::F(vec![321.0])],
                vec![Value::F(vec![123.0])],
                vec![Value::F(vec![67.0])],
                vec![Value::U(0)],
                vec![Value::S(1)],
                vec![Value::U(5)],
                vec![Value::F(vec![42.0])],
            ],
        };
        let array = Object {
            class: 1,
            number: 1,
            fields: vec![vec![Value::Object(None), Value::Object(Some(Box::new(camera)))]],
        };
        Xfs {
            minor: 0,
            object_count: 3,
            classes: vec![
                class("rCameraParamList", &[("mParamList", Type::ClassRef)]),
                class("MtArray", &[("mpArray", Type::ClassRef)]),
                class(
                    "cCamExParamNml",
                    &[
                        ("mDistance", Type::F32),
                        ("mHeight", Type::F32),
                        ("mFov", Type::F32),
                        ("mCameraTarget", Type::U32),
                        ("mScrAdjustType", Type::S32),
                        ("mScrAdjustFlag", Type::U32),
                        ("mScrAdjustHeight", Type::F32),
                    ],
                ),
            ],
            root: Object {
                class: 0,
                number: 0,
                fields: vec![vec![Value::Object(Some(Box::new(array)))]],
            },
        }
    }

    #[test]
    fn preserves_null_slots_and_reads_authored_values() {
        let parameters = Parameters::from_xfs(&fixture()).unwrap();
        assert_eq!(
            parameters.normal,
            vec![
                None,
                Some(Normal {
                    distance: 321.0,
                    height: 123.0,
                    fov_degrees: 67.0,
                    camera_target: 0,
                    screen_adjust_type: 1,
                    screen_adjust_flags: 5,
                    screen_adjust_height: 42.0,
                })
            ]
        );
    }

    #[test]
    fn missing_fields_are_errors_and_other_camera_types_keep_their_slot() {
        let mut xfs = fixture();
        xfs.classes[2].properties[1].name = "unrecognized".into();
        assert!(Parameters::from_xfs(&xfs).is_err());
        xfs.classes[2].hash = mtarc::type_hash("cCamExParamTps");
        assert_eq!(Parameters::from_xfs(&xfs).unwrap().normal, vec![None, None]);
        assert!(Parameters::parse(b"XFS").is_err());
    }

    #[test]
    fn collision_anchor_respects_pitch_boundary_and_authored_modes() {
        let mut normal = Parameters::from_xfs(&fixture()).unwrap().normal[1].unwrap();
        let target = [20.0, 80.0, 30.0];
        let actor = Some(([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]));
        assert_eq!(
            normal.collision_anchor(target, actor, -0.8, true, false),
            Some([1.0, 80.0, 3.0])
        );
        assert_eq!(
            normal.collision_anchor(target, actor, -0.81, true, false),
            Some([4.0, 128.0, 6.0])
        );
        assert_eq!(
            normal.collision_anchor(target, actor, 0.0, true, true),
            Some([4.0, 128.0, 6.0])
        );
        normal.screen_adjust_flags = 1;
        assert_eq!(
            normal.collision_anchor(target, actor, -0.81, false, false),
            Some([1.0, 44.0, 3.0])
        );
        normal.screen_adjust_type = 2;
        assert_eq!(
            normal.collision_anchor(target, actor, 0.0, true, false),
            Some([4.0, 5.0, 6.0])
        );
        assert_eq!(
            normal.collision_anchor(target, None, -1.0, true, true),
            Some(target)
        );
        normal.screen_adjust_flags = 0;
        assert_eq!(normal.collision_anchor(target, actor, -1.0, true, true), None);
    }
}
