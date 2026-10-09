use ddquest::sdl::{Mode, Scheduler, Value};
use mtcamera::CameraList;
use std::{collections::BTreeSet, path::PathBuf};

fn install() -> Option<ddvfs::Vfs> {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let path = explicit
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !path.exists() {
        assert!(
            explicit.is_none(),
            "explicit DDDA_NATIVE_PC does not exist: {}",
            path.display()
        );
        eprintln!("install test skipped: {} unavailable", path.display());
        return None;
    }
    Some(ddvfs::Vfs::open(path).unwrap())
}

#[test]
fn every_installed_camera_list_copy() {
    let Some(vfs) = install() else {
        return;
    };
    let hash = mtarc::type_hash("rCameraList");
    let (mut paths, mut copies, mut slots, mut cameras, mut frames) = (0, 0, 0, 0, 0usize);
    let mut metadata = BTreeSet::new();
    let mut fov_types = BTreeSet::new();
    for path in vfs.paths_of_type(hash) {
        paths += 1;
        for loc in vfs.locate(path, hash).unwrap() {
            copies += 1;
            let b = vfs.read_at(loc).unwrap();
            let list = CameraList::parse(&b).unwrap_or_else(|e| panic!("{path}: {e}"));
            slots += list.cameras.len();
            for camera in list.cameras.iter().flatten() {
                cameras += 1;
                frames += camera.frames();
                metadata.insert((camera.user_data, camera.opaque_18));
                fov_types.insert(camera.fov_type);
                for index in 0..camera.frames() {
                    let sample = camera.frame(index).unwrap();
                    assert!(
                        sample
                            .position
                            .iter()
                            .chain(&sample.target)
                            .chain(&sample.rotation)
                            .chain(std::iter::once(&sample.fov_degrees))
                            .all(|v| v.is_finite()),
                        "{path} frame {index}"
                    );
                    let norm = sample.rotation.iter().map(|v| v * v).sum::<f32>();
                    assert!((norm - 1.0).abs() < 1e-4, "{path} quaternion {index}: {norm}");
                    let pose = camera.sample(index as f32 + 0.5, false).unwrap().pose();
                    assert!(pose
                        .position
                        .iter()
                        .chain(&pose.target)
                        .chain(&pose.up)
                        .chain(std::iter::once(&pose.fov_degrees))
                        .all(|v| v.is_finite()));
                }
            }
        }
    }
    assert!(copies > 0, "install has no camera lists");
    eprintln!("LCM: {paths} paths, {copies} copies, {slots} slots, {cameras} cameras, {frames} frames; {} metadata tuples; fov types {fov_types:?}", metadata.len());
}

#[test]
fn st200_ev10_timeline_camera_resource_and_cuts_resolve() {
    let Some(vfs) = install() else {
        return;
    };
    let bytes = vfs
        .read(r"event\st200\ev10\st200ev10", mtarc::type_hash("rScheduler"))
        .unwrap();
    let timeline = Scheduler::parse(&bytes).unwrap();
    let resources: Vec<_> = timeline
        .tracks
        .iter()
        .flat_map(|t| &t.keys)
        .filter_map(|k| match &k.value {
            Value::Resource(Some((hash, path))) if *hash == mtarc::type_hash("rCameraList") => {
                Some(path.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(resources, [r"event\st200\ev10\list\camera\camera"]);
    let bytes = vfs.read(resources[0], mtarc::type_hash("rCameraList")).unwrap();
    let cameras = CameraList::parse(&bytes).unwrap();
    assert_eq!(cameras.cameras.iter().flatten().count(), 8);
    let cuts = &timeline.tracks[timeline.cut_track as usize];
    assert_eq!(cuts.name, "mCut");
    assert_eq!(
        cuts.keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
        [0, 460, 670, 790, 910, 1030, 1150, 1270]
    );
    let frame_track = timeline
        .tracks
        .iter()
        .find(|t| t.name == "mFrame" && t.link == cuts.link)
        .unwrap();
    for (index, key) in cuts.keys.iter().enumerate() {
        assert_eq!(key.value, Value::Int(index as i32));
        let camera = cameras.cameras[index].as_ref().unwrap();
        let end = cuts.keys.get(index + 1).map_or(timeline.frames, |k| k.frame);
        let frame_key = Scheduler::key_at(frame_track, key.frame).unwrap();
        assert_eq!(frame_key.frame, key.frame);
        assert_eq!(frame_key.mode, Mode::Count(1));
        assert_eq!(frame_key.value, Value::Float(0.0));
        assert_eq!(camera.frames(), [460, 210, 120, 120, 120, 120, 120, 433][index]);
        assert!(camera.frames() >= (end - key.frame) as usize);
        eprintln!(
            "cut {index}: SDL {}..{end}, LCM {} frames, mFrame {:?}/{:?}",
            key.frame,
            camera.frames(),
            frame_key.mode,
            frame_key.value
        );
        assert!(camera.frames() > 0);
        assert!(camera.sample(0.0, false).is_some());
    }
}
#[test]
fn st200_ev10_uses_constructor_projection_defaults() {
 let Some(vfs) = install() else { return; };
 let bytes = vfs.read(r"event\st200\ev10\st200ev10", mtarc::type_hash("rScheduler")).unwrap();
 let timeline=Scheduler::parse(&bytes).unwrap();
 assert!(!timeline.tracks.iter().any(|t| ["mNearPlane","mFarPlane","mZoom","mAspect","mFovType","mFov"].contains(&t.name.as_str())));
}
