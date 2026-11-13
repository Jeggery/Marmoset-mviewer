use super::*;
use serde_json::{Value, json};
use std::fs;

fn snapshot() -> JsExportScene {
    serde_json::from_value(json!({
        "source": "test", "version": 1, "selectedAnimationIndex": -1,
        "selectedCameraIndex": -1
    }))
    .unwrap()
}

fn fixture() -> (Vec<u8>, Scene) {
    let scene: Scene = serde_json::from_value(json!({
        "materials": [{"name": "plain", "albedoTex": ""}],
        "meshes": (0..3).map(|i| json!({
            "name": format!("mesh{i}"), "file": "mesh.dat", "vertexCount": 3,
            "indexCount": 3, "indexTypeSize": 2, "wireCount": 0,
            "subMeshes": [{"material": "plain", "firstIndex": 0, "indexCount": 3}]
        })).collect::<Vec<_>>(),
        "Cameras": {"Existing": {
            "view": {"fov": 45, "angles": [10, 20], "pivot": [1, 2, 3], "orbitRadius": 5},
            "post": {"grain": 0.2}
        }}
    }))
    .unwrap();
    let mut mesh = vec![0, 0, 1, 0, 2, 0];
    for position in [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position.into_iter().chain([0.0, 0.0]) {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
        for value in [32767_u16, 16384, 16384, 32767, 16384, 16384] {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut archive = Vec::new();
    append_entry(
        &mut archive,
        "scene.json",
        &serde_json::to_vec(&scene).unwrap(),
    );
    append_entry(&mut archive, "mesh.dat", &mesh);
    (archive, scene)
}

fn append_entry(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(b"\0bin\0");
    for value in [0, data.len() as u32, data.len() as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(data);
}

fn options(format: GuiExportFormat, indices: Vec<usize>) -> GuiExportOptions {
    GuiExportOptions {
        format,
        included_meshes: indices,
        include_textures: false,
        include_animations: true,
        include_cameras: true,
        include_lights: true,
    }
}

fn exported_json(directory: &Path, format: &GuiExportFormat) -> Value {
    match format {
        GuiExportFormat::Gltf => {
            serde_json::from_slice(&fs::read(directory.join("scene.gltf")).unwrap()).unwrap()
        }
        GuiExportFormat::Glb => {
            let bytes = fs::read(directory.join("scene.glb")).unwrap();
            let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
            serde_json::from_slice(&bytes[20..20 + size]).unwrap()
        }
        GuiExportFormat::Obj => unreachable!(),
    }
}

#[test]
fn selected_source_indices_export_the_same_meshes_in_every_format() {
    let (bytes, _) = fixture();
    let mut js = snapshot();
    js.archive_base64 = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
    for indices in [vec![1], vec![0, 2], vec![2, 0, 2], vec![0, 1, 2]] {
        let expected: Vec<_> = (0..3)
            .filter(|i| indices.contains(i))
            .map(|i| format!("mesh{i}"))
            .collect();
        for format in [
            GuiExportFormat::Obj,
            GuiExportFormat::Gltf,
            GuiExportFormat::Glb,
        ] {
            let output = tempfile::tempdir().unwrap();
            let report = export_from_js_scene_path_with_options_and_progress(
                Path::new("scene.mview"),
                output.path(),
                &js,
                &options(format.clone(), indices.clone()),
                |_, _| {},
            )
            .unwrap();
            let actual: Vec<String> = if matches!(format, GuiExportFormat::Obj) {
                let obj = fs::read_to_string(output.path().join("scene.obj")).unwrap();
                assert_eq!(
                    obj.lines().filter(|l| l.starts_with("v ")).count(),
                    expected.len() * 3
                );
                assert_eq!(
                    obj.lines().filter(|l| l.starts_with("f ")).count(),
                    expected.len()
                );
                obj.lines()
                    .filter_map(|line| line.strip_prefix("o "))
                    .map(str::to_owned)
                    .collect()
            } else {
                let doc = exported_json(output.path(), &format);
                doc["meshes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|mesh| {
                        assert_eq!(mesh["primitives"][0]["mode"], 4);
                        mesh["name"].as_str().unwrap().to_owned()
                    })
                    .collect()
            };
            assert_eq!(actual, expected);
            assert_eq!(report.exported_meshes, actual.len());
            assert_eq!(report.total_meshes, 3);
        }
    }
}

#[test]
fn empty_and_out_of_range_selections_fail_without_writing_output() {
    let (bytes, _) = fixture();
    let mut js = snapshot();
    js.archive_base64 = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
    for indices in [vec![], vec![3], vec![0, usize::MAX]] {
        for format in [
            GuiExportFormat::Obj,
            GuiExportFormat::Gltf,
            GuiExportFormat::Glb,
        ] {
            let output = tempfile::tempdir().unwrap();
            assert!(
                export_from_js_scene_path_with_options_and_progress(
                    Path::new("scene.mview"),
                    output.path(),
                    &js,
                    &options(format, indices.clone()),
                    |_, _| {},
                )
                .is_err()
            );
            assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn no_options_helpers_still_export_all_meshes() {
    let (bytes, _) = fixture();
    let input = tempfile::tempdir().unwrap();
    let path = input.path().join("scene.mview");
    fs::write(&path, bytes).unwrap();
    for with_progress in [false, true] {
        let output = tempfile::tempdir().unwrap();
        let report = if with_progress {
            export_from_js_scene_path_with_progress(&path, output.path(), &snapshot(), |_, _| {})
        } else {
            export_from_js_scene_path(&path, output.path(), &snapshot())
        }
        .unwrap();
        assert_eq!(report.exported_meshes, 3);
        assert_eq!(
            exported_json(output.path(), &GuiExportFormat::Gltf)["meshes"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn filtering_keeps_animation_mesh_ids_aligned_with_source_selection() {
    let (_, mut scene) = fixture();
    scene.anim_data = Some(serde_json::from_value(json!({
        "numMatrices": 0, "sceneScale": 1, "meshIDs": [{"partIndex": 8}, {"partIndex": 4}, {"partIndex": 9}]
    })).unwrap());
    let selection =
        build_export_options(&scene, &options(GuiExportFormat::Gltf, vec![0, 2])).unwrap();
    let filtered = crate::filtered_scene(&scene, &selection);
    assert_eq!(
        filtered
            .meshes
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["mesh0", "mesh2"]
    );
    assert_eq!(
        filtered
            .anim_data
            .unwrap()
            .mesh_ids
            .iter()
            .map(|m| m.part_index)
            .collect::<Vec<_>>(),
        [8, 9]
    );
}

#[test]
fn camera_snapshot_preserves_source_description_and_updates_valid_fields() {
    let (_, mut scene) = fixture();
    scene.main_camera = scene.cameras.get("Existing").cloned();
    let before = serde_json::to_value(&scene).unwrap();
    let mut js = snapshot();
    js.cameras = serde_json::from_value(json!([
        {"index": 0, "name": "Existing", "fov": 60, "near": 0.2, "far": 100},
        {"index": 1, "name": "Main Camera"},
        {"index": 2, "name": "New Camera", "fov": 30}
    ]))
    .unwrap();
    let merged = merge_js_export_scene(&scene, &js);
    let camera = &merged.cameras["Existing"];
    assert_eq!(camera.view.as_ref().unwrap().fov, Some(60.0));
    assert_eq!(camera.view.as_ref().unwrap().angles, Some([10.0, 20.0]));
    assert_eq!(camera.view.as_ref().unwrap().pivot, Some([1.0, 2.0, 3.0]));
    assert_eq!(camera.view.as_ref().unwrap().orbit_radius, Some(5.0));
    assert_eq!(camera.post.as_ref().unwrap().grain, Some(0.2));
    assert_eq!(camera.near, Some(0.2));
    assert_eq!(camera.far, Some(100.0));
    assert_eq!(
        merged
            .main_camera
            .as_ref()
            .unwrap()
            .view
            .as_ref()
            .unwrap()
            .fov,
        Some(45.0)
    );
    assert!(!merged.cameras.contains_key("Main Camera"));
    assert_eq!(
        merged.cameras["New Camera"].view.as_ref().unwrap().fov,
        Some(30.0)
    );
    assert_eq!(
        serde_json::to_value(&scene).unwrap(),
        before,
        "merging must not mutate the source"
    );
    js.cameras[0].fov = Some(f32::NAN);
    js.cameras[0].near = Some(-1.0);
    js.cameras[0].far = Some(-10.0);
    let again = merge_js_export_scene(&merged, &js);
    assert_eq!(
        serde_json::to_value(&again.cameras["Existing"]).unwrap(),
        serde_json::to_value(camera).unwrap()
    );
    // A snapshot may introduce the main camera when the source has none.
    scene.main_camera = None;
    js.cameras[1].fov = Some(50.0);
    let merged = merge_js_export_scene(&scene, &js);
    assert_eq!(merged.main_camera.unwrap().view.unwrap().fov, Some(50.0));
    assert!(!merged.cameras.contains_key("Main Camera"));
}

#[test]
fn merged_static_and_animated_cameras_export_projection_and_transforms() {
    let (bytes, _) = fixture();
    let mut js = snapshot();
    js.archive_base64 = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
    js.cameras = serde_json::from_value(json!([{
        "index": 0, "name": "Existing", "fov": 60, "near": 0.2, "far": 100,
        "transform": [1,0,0,0, 0,1,0,0, 0,0,1,0, 7,8,9,1]
    }]))
    .unwrap();
    for format in [GuiExportFormat::Gltf, GuiExportFormat::Glb] {
        let output = tempfile::tempdir().unwrap();
        export_from_js_scene_path_with_options_and_progress(
            Path::new("scene.mview"),
            output.path(),
            &js,
            &options(format.clone(), vec![1]),
            |_, _| {},
        )
        .unwrap();
        let doc = exported_json(output.path(), &format);
        assert_eq!(doc["cameras"].as_array().unwrap().len(), 1);
        let camera = &doc["cameras"][0]["perspective"];
        assert!((camera["yfov"].as_f64().unwrap() - 60_f64.to_radians()).abs() < 1e-6);
        assert!((camera["znear"].as_f64().unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(camera["zfar"].as_f64(), Some(100.0));
        let node = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["camera"].is_number())
            .unwrap();
        assert_eq!(
            serde_json::from_value::<[f64; 3]>(node["translation"].clone()).unwrap(),
            [7.0, 8.0, 9.0]
        );
    }
    // An animated camera's node transform must remain driven by its clip, not
    // be overwritten by the static world transform in the snapshot.
    let (_, scene) = fixture();
    let merged = merge_js_export_scene(&scene, &js);
    let mut builder = crate::gltf::GltfBuilder::default();
    let node = builder.add_runtime_node(crate::gltf::NodeDef {
        name: Some("Existing".into()),
        translation: Some([1.0, 2.0, 3.0]),
        mesh: None,
        skin: None,
        matrix: None,
        rotation: None,
        scale: None,
        children: None,
        camera: None,
        extensions: None,
        extras: None,
    });
    assert!(builder.attach_runtime_camera_node(node, "Existing", &merged.cameras["Existing"]));
    assert_eq!(builder.nodes[node].translation, Some([1.0, 2.0, 3.0]));
    assert_eq!(builder.cameras[0].perspective.zfar, Some(100.0));
}
