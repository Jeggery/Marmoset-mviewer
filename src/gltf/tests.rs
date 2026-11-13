use super::*;

#[test]
fn animated_exports_omit_leaf_children_and_preserve_hierarchy_skins_and_clips() {
    assert_animated_exports(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/vivfox.mview"
        )),
        7,
    );
}

#[test]
#[ignore = "requires the downloaded Fedor sample; set FEDOR_MVIEW to its local path"]
fn fedor_animated_exports() {
    let path =
        std::env::var_os("FEDOR_MVIEW").expect("set FEDOR_MVIEW to the downloaded fedor.mview");
    assert_animated_exports(Path::new(&path), 2);
}

#[test]
fn natnephilim_exports_preserve_four_rigs_and_clip_components() {
    assert_animated_exports(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/natnephilim.mview"
        )),
        1,
    );
}

fn assert_animated_exports(input: &Path, expected_clips: usize) {
    let archive = Archive::from_path(input).unwrap();
    let scene = Scene::from_bytes(&archive.get("scene.json").unwrap().data).unwrap();
    let animations = ParsedAnimationSet::from_scene(&archive, &scene)
        .unwrap()
        .unwrap();
    let primary = &animations.animations[scene
        .anim_data
        .as_ref()
        .unwrap()
        .selected_animation
        .unwrap_or(0)];
    let object_count = primary.animated_objects.len();
    for (format, extension) in [
        (GltfOutputFormat::Gltf, "gltf"),
        (GltfOutputFormat::Glb, "glb"),
    ] {
        let output = tempfile::tempdir().unwrap();
        export_scene_with_js_scene_format_progress(
            &archive,
            &scene,
            input,
            output.path(),
            None,
            format,
            &mut |_, _| {},
        )
        .unwrap();
        let file = output
            .path()
            .join(input.file_stem().unwrap())
            .with_extension(extension);
        let bytes = fs::read(file).unwrap();
        let json = match format {
            GltfOutputFormat::Gltf => bytes.as_slice(),
            GltfOutputFormat::Glb => {
                let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
                &bytes[20..20 + size]
            }
        };
        let binary = match format {
            GltfOutputFormat::Gltf => fs::read(
                output
                    .path()
                    .join(input.file_stem().unwrap())
                    .with_extension("bin"),
            )
            .unwrap(),
            GltfOutputFormat::Glb => bytes[28 + json.len()..].to_vec(),
        };
        let doc: serde_json::Value = serde_json::from_slice(json).unwrap();
        let nodes = doc["nodes"].as_array().unwrap();
        assert!(nodes.len() >= object_count + scene.meshes.len());
        let mut leaves = 0;
        for node in nodes {
            if let Some(children) = node.get("children") {
                let children = children.as_array().unwrap();
                assert!(!children.is_empty(), "empty children on {}", node["name"]);
                assert!(
                    children
                        .iter()
                        .all(|child| (child.as_u64().unwrap() as usize) < nodes.len())
                );
            } else {
                leaves += 1;
            }
        }
        assert!(leaves > 0);
        for (index, object) in primary.animated_objects.iter().enumerate() {
            let parent = object.desc.parent_index;
            if parent != index && parent < object_count {
                assert!(
                    nodes[parent]["children"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(index))
                );
            } else {
                assert!(
                    doc["scenes"][0]["nodes"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(index))
                );
            }
        }
        assert!(nodes.iter().any(|node| node["skin"].is_number()));
        let skins = doc["skins"].as_array().unwrap();
        assert!(!skins.is_empty());
        let assert_untargeted = |accessor: &serde_json::Value| {
            let accessor = &doc["accessors"][accessor.as_u64().unwrap() as usize];
            let view = &doc["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
            assert!(
                view.get("target").is_none(),
                "animation/skin data must not have a GPU buffer target"
            );
        };
        for skin in skins {
            assert_untargeted(&skin["inverseBindMatrices"]);
            for joint in skin["joints"].as_array().unwrap() {
                let node = &nodes[joint.as_u64().unwrap() as usize];
                assert!(node.get("translation").is_some());
                assert!(node.get("rotation").is_some());
                assert!(node.get("scale").is_some());
            }
        }
        for (index, node) in nodes.iter().enumerate() {
            if node["skin"].is_number() {
                assert!(
                    doc["scenes"][0]["nodes"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(index)),
                    "skinned render node must be a root"
                );
            }
        }
        let clips = doc["animations"].as_array().unwrap();
        assert_eq!(clips.len(), expected_clips);
        for (clip, source) in clips.iter().zip(&animations.animations) {
            assert_eq!(clip["name"].as_str().unwrap(), source.desc.name);
            assert!(!clip["channels"].as_array().unwrap().is_empty());
            assert!(!clip["samplers"].as_array().unwrap().is_empty());
            for joint in skins
                .iter()
                .flat_map(|skin| skin["joints"].as_array().unwrap())
            {
                let paths: Vec<_> = clip["channels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|channel| channel["target"]["node"] == *joint)
                    .map(|channel| channel["target"]["path"].as_str().unwrap())
                    .collect();
                assert_eq!(
                    paths.len(),
                    3,
                    "retain constant joint components when switching clips"
                );
            }
            for channel in clip["channels"].as_array().unwrap() {
                let target = channel["target"]["node"].as_u64().unwrap() as usize;
                assert!(target < nodes.len());
                assert!(
                    nodes[target].get("matrix").is_none(),
                    "TRS animation must not target a matrix node"
                );
            }
            for sampler in clip["samplers"].as_array().unwrap() {
                assert_untargeted(&sampler["input"]);
                assert_untargeted(&sampler["output"]);
                let input = &doc["accessors"][sampler["input"].as_u64().unwrap() as usize];
                let view = &doc["bufferViews"][input["bufferView"].as_u64().unwrap() as usize];
                let start = view["byteOffset"].as_u64().unwrap() as usize
                    + input["byteOffset"].as_u64().unwrap() as usize;
                let count = input["count"].as_u64().unwrap() as usize;
                let last = f32::from_le_bytes(
                    binary[start + 4 * (count - 1)..start + 4 * count]
                        .try_into()
                        .unwrap(),
                );
                assert!(
                    (last - source.desc.length).abs() < 1e-4,
                    "{} duration {last} != {}",
                    source.desc.name,
                    source.desc.length
                );
            }
        }
    }
}

#[test]
fn hdr_emission_preserves_intensity_with_the_emissive_strength_extension() {
    let archive = Archive::from_bytes(&[]).unwrap();
    let mut builder = GltfBuilder::default();
    for intensity in [0.0, 0.5, 1.0, 2.727, 4.0] {
        let material: MaterialDesc = serde_json::from_value(json!({
            "name": "emissive", "albedoTex": "", "emissiveIntensity": intensity,
            "extrasTexCoordRanges": {"emissiveTex": {"scaleBias": [1, 1, 0, 0]}}
        }))
        .unwrap();
        builder
            .add_material(&archive, &material, &mut HashMap::new())
            .unwrap();
        let exported = serde_json::to_value(builder.materials.last().unwrap()).unwrap();
        let factor = exported["emissiveFactor"][0].as_f64().unwrap();
        assert!(exported["emissiveTexture"]["index"].is_number());
        assert!((0.0..=1.0).contains(&factor));
        let strength =
            exported["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"]
                .as_f64()
                .unwrap_or(1.0);
        assert!((factor * strength - intensity).abs() < 1e-5);
        assert_eq!(exported.get("extensions").is_some(), intensity > 1.0);
    }
    assert_eq!(
        builder
            .extensions_used
            .iter()
            .filter(|s| *s == "KHR_materials_emissive_strength")
            .count(),
        1
    );
    assert_eq!(
        builder
            .extensions_used
            .iter()
            .filter(|s| *s == "KHR_texture_transform")
            .count(),
        1
    );
}

#[test]
fn emissive_intensity_without_an_emissive_region_does_not_light_the_surface() {
    let (archive, _) = export_fixture();
    let material: MaterialDesc = serde_json::from_value(json!({
        "name": "non-emissive", "albedoTex": "d.png", "emissiveIntensity": 6.4
    }))
    .unwrap();
    let mut builder = GltfBuilder::default();
    builder
        .add_material(&archive, &material, &mut HashMap::new())
        .unwrap();
    let material = serde_json::to_value(&builder.materials[0]).unwrap();
    assert_eq!(material["emissiveFactor"], json!([0.0, 0.0, 0.0]));
    assert!(material.get("emissiveTexture").is_none());
    assert!(material.get("extensions").is_none());
}

#[test]
fn emissive_atlas_preserves_full_resolution_and_color_encoding() {
    let mut atlas = image::RgbaImage::from_pixel(2, 4, image::Rgba([255, 255, 255, 255]));
    // A bottom-half tile in image coordinates is the lower half in WebGL UVs.
    atlas.put_pixel(0, 2, image::Rgba([0, 0, 0, 255]));
    atlas.put_pixel(1, 2, image::Rgba([128, 0, 0, 255]));
    atlas.put_pixel(0, 3, image::Rgba([0, 128, 0, 255]));
    atlas.put_pixel(1, 3, image::Rgba([0, 0, 128, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    atlas
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let converted = convert_emissive(Some(&encoded.into_inner())).unwrap();
    let output = image::load_from_memory(&converted).unwrap().to_rgba8();
    assert_eq!(output.dimensions(), (2, 4));
    assert_eq!(output.get_pixel(0, 0).0, [255, 255, 255, 255]);
    assert_eq!(output.get_pixel(0, 2).0, [0, 0, 0, 255]);
    // sRGB encoding of (128/255)^2 is about 137, not 128.
    assert_eq!(output.get_pixel(1, 2).0, [137, 0, 0, 255]);
    assert_eq!(output.get_pixel(0, 3).0, [0, 137, 0, 255]);
    assert_eq!(output.get_pixel(1, 3).0, [0, 0, 137, 255]);
}

#[test]
fn emissive_transform_converts_bottom_left_atlas_offset_to_gltf() {
    let archive = Archive::from_bytes(&[]).unwrap();
    let material: MaterialDesc = serde_json::from_value(json!({
        "name":"atlas", "albedoTex":"", "extrasTexCoordRanges": {
            "emissiveTex": {"scaleBias":[0.5,0.25,0.125,0.125]}
        }
    }))
    .unwrap();
    let mut builder = GltfBuilder::default();
    builder
        .add_material(&archive, &material, &mut HashMap::new())
        .unwrap();
    let value = serde_json::to_value(&builder.materials[0]).unwrap();
    let texture = &value["emissiveTexture"];
    assert_eq!(texture["texCoord"], 2);
    assert_eq!(
        texture["extensions"]["KHR_texture_transform"]["scale"],
        json!([0.5, 0.25])
    );
    assert_eq!(
        texture["extensions"]["KHR_texture_transform"]["offset"],
        json!([0.125, 0.625])
    );
}

#[test]
fn emissive_atlas_export_declares_transform_and_contiguous_uv_sets_in_both_formats() {
    let (archive, mut scene) = export_fixture();
    scene.materials[0].extras_tex = Some("s.png".into());
    scene.materials[0].extras_tex_coord_ranges = Some(
        serde_json::from_value(json!({
            "emissiveTex":{"scaleBias":[0.5,0.5,0.25,0.25]}
        }))
        .unwrap(),
    );
    for (format, extension) in [
        (GltfOutputFormat::Gltf, "gltf"),
        (GltfOutputFormat::Glb, "glb"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        export_scene_with_js_scene_format_progress(
            &archive,
            &scene,
            Path::new("atlas.mview"),
            dir.path(),
            None,
            format,
            &mut |_, _| {},
        )
        .unwrap();
        let bytes = fs::read(dir.path().join(format!("atlas.{extension}"))).unwrap();
        let doc: serde_json::Value = match format {
            GltfOutputFormat::Gltf => serde_json::from_slice(&bytes).unwrap(),
            GltfOutputFormat::Glb => {
                let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
                serde_json::from_slice(&bytes[20..20 + size]).unwrap()
            }
        };
        assert!(
            doc["extensionsRequired"]
                .as_array()
                .unwrap()
                .contains(&json!("KHR_texture_transform"))
        );
        let tex = &doc["materials"][0]["emissiveTexture"];
        assert_eq!(tex["texCoord"], 2);
        let attrs = &doc["meshes"][0]["primitives"][0]["attributes"];
        for name in ["TEXCOORD_0", "TEXCOORD_1", "TEXCOORD_2"] {
            assert!(attrs[name].is_number());
        }
        let image_index = doc["textures"][tex["index"].as_u64().unwrap() as usize]["source"]
            .as_u64()
            .unwrap() as usize;
        let image = &doc["images"][image_index];
        let image_bytes = match format {
            GltfOutputFormat::Gltf => {
                fs::read(dir.path().join(image["uri"].as_str().unwrap())).unwrap()
            }
            GltfOutputFormat::Glb => {
                let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
                let view = &doc["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
                let start = 28 + size + view["byteOffset"].as_u64().unwrap() as usize;
                bytes[start..start + view["byteLength"].as_u64().unwrap() as usize].to_vec()
            }
        };
        assert_eq!(
            image::load_from_memory(&image_bytes)
                .unwrap()
                .to_rgba8()
                .dimensions(),
            (2, 1)
        );
    }
}

#[test]
fn shared_material_repeat_seam_uses_one_image_without_splitting_any_mesh() {
    let (archive, mut scene) = export_fixture();
    scene.materials[0].extras_tex = Some("s.png".into());
    scene.materials[0].extras_tex_coord_ranges = Some(
        serde_json::from_value(json!({
            "emissiveTex":{"scaleBias":[0.5,0.5,0.25,0.25]}
        }))
        .unwrap(),
    );
    let mut repeat_mesh = archive.get("mesh.dat").unwrap().data.clone();
    for (i, uv) in [[0.8_f32, 0.0], [1.2, 0.2], [0.9, 0.4]].iter().enumerate() {
        let offset = 6 + i * 32 + 12;
        repeat_mesh[offset..offset + 4].copy_from_slice(&uv[0].to_le_bytes());
        repeat_mesh[offset + 4..offset + 8].copy_from_slice(&uv[1].to_le_bytes());
    }
    let mut bytes = Vec::new();
    for entry in archive.entries() {
        append_entry(&mut bytes, &entry.name, &entry.data);
    }
    append_entry(&mut bytes, "repeat.dat", &repeat_mesh);
    let archive = Archive::from_bytes(&bytes).unwrap();
    let mut second = scene.meshes[0].clone();
    second.name = "repeat triangle".into();
    second.file = "repeat.dat".into();
    scene.meshes.push(second);
    // The first mesh is safe; only the second crosses a seam. Both must share
    // the same material-wide decision, independent of mesh visitation order.
    for format in [GltfOutputFormat::Gltf, GltfOutputFormat::Glb] {
        let dir = tempfile::tempdir().unwrap();
        export_scene_with_js_scene_format_progress(
            &archive,
            &scene,
            Path::new("shared.mview"),
            dir.path(),
            None,
            format,
            &mut |_, _| {},
        )
        .unwrap();
        let json: serde_json::Value = match format {
            GltfOutputFormat::Gltf => {
                serde_json::from_slice(&fs::read(dir.path().join("shared.gltf")).unwrap()).unwrap()
            }
            GltfOutputFormat::Glb => {
                let bytes = fs::read(dir.path().join("shared.glb")).unwrap();
                let size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
                serde_json::from_slice(&bytes[20..20 + size]).unwrap()
            }
        };
        assert_eq!(json["materials"].as_array().unwrap().len(), 1);
        let tex = &json["materials"][0]["emissiveTexture"];
        assert!(tex.get("extensions").is_none());
        assert!(tex.get("texCoord").is_none());
        let images = json["images"].as_array().unwrap();
        assert_eq!(
            images
                .iter()
                .filter(|i| i["name"].as_str().unwrap().contains("emissive"))
                .count(),
            1
        );
        assert!(
            images
                .iter()
                .all(|i| !i["name"].as_str().unwrap().contains("repeat"))
        );
        assert!(json.get("extensionsRequired").is_none());
        for mesh in json["meshes"].as_array().unwrap() {
            assert_eq!(mesh["primitives"].as_array().unwrap().len(), 1);
            let primitive = &mesh["primitives"][0];
            assert_eq!(primitive["material"], 0);
            assert!(primitive["attributes"].get("TEXCOORD_2").is_none());
        }
    }
}

fn append_entry(archive: &mut Vec<u8>, name: &str, bytes: &[u8]) {
    archive.extend_from_slice(name.as_bytes());
    archive.extend_from_slice(b"\0application/octet-stream\0");
    archive.extend_from_slice(&0_u32.to_le_bytes());
    archive.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    archive.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    archive.extend_from_slice(bytes);
}

fn png(color: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba(color));
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn export_fixture() -> (Archive, Scene) {
    let description = json!({
        "materials": [{"name":"gold", "albedoTex":"d.png", "reflectivityTex":"s.png", "glossTex":"g.png", "normalTex":"maps/n.png", "ggxSpecular":true}],
        "meshes": [{"name":"triangle", "file":"mesh.dat", "vertexCount":3, "indexCount":3,
            "indexTypeSize":2, "wireCount":0, "subMeshes":[{"material":"gold", "firstIndex":0,"indexCount":3}]}]
    });
    let mut mesh = vec![0, 0, 1, 0, 2, 0];
    for position in [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for value in position.into_iter().chain([0.0, 0.0]) {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
        for value in [32767_u16, 16384, 16384, 32767, 16384, 16384] {
            mesh.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut bytes = Vec::new();
    append_entry(&mut bytes, "mesh.dat", &mesh);
    append_entry(&mut bytes, "d.png", &png([0, 0, 0, 255]));
    let mut specular = image::RgbaImage::from_pixel(2, 1, image::Rgba([230, 180, 90, 255]));
    specular.put_pixel(1, 0, image::Rgba([30, 30, 30, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(specular)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    append_entry(&mut bytes, "s.png", &encoded.into_inner());
    append_entry(&mut bytes, "g.png", &png([200, 200, 200, 255]));
    // A non-neutral constant normal has no equivalent glTF factor and must stay.
    append_entry(&mut bytes, "maps/n.png", &png([160, 128, 255, 255]));
    append_entry(&mut bytes, "unused.png", &png([1, 2, 3, 255]));
    let archive = Archive::from_bytes(&bytes).unwrap();
    let scene = Scene::from_bytes(&serde_json::to_vec(&description).unwrap()).unwrap();
    (archive, scene)
}

#[test]
fn glb_embeds_converted_material_and_authored_tangent_frame() {
    let (archive, scene) = export_fixture();
    let output = tempfile::tempdir().unwrap();
    export_scene_with_js_scene_format_progress(
        &archive,
        &scene,
        Path::new("triangle.mview"),
        output.path(),
        None,
        GltfOutputFormat::Glb,
        &mut |_, _| {},
    )
    .unwrap();
    let files: Vec<_> = fs::read_dir(output.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, vec![std::ffi::OsString::from("triangle.glb")]);
    let glb = fs::read(output.path().join("triangle.glb")).unwrap();
    let json_size = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_size]).unwrap();
    let binary = &glb[28 + json_size..];
    assert!(doc["meshes"][0]["primitives"][0]["attributes"]["TANGENT"].is_number());
    assert!(doc["materials"][0]["extensions"]["KHR_materials_pbrSpecularGlossiness"].is_null());
    let mr = &doc["materials"][0]["pbrMetallicRoughness"]["metallicRoughnessTexture"];
    let texture = mr["index"].as_u64().unwrap() as usize;
    let image = doc["textures"][texture]["source"].as_u64().unwrap() as usize;
    for image in doc["images"].as_array().unwrap() {
        assert!(image["uri"].is_null());
        let view = &doc["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
        assert!(view["target"].is_null());
    }
    let view = &doc["bufferViews"][doc["images"][image]["bufferView"].as_u64().unwrap() as usize];
    let offset = view["byteOffset"].as_u64().unwrap() as usize;
    let length = view["byteLength"].as_u64().unwrap() as usize;
    let packed = image::load_from_memory(&binary[offset..offset + length])
        .unwrap()
        .to_rgba8();
    assert_eq!(packed.get_pixel(0, 0)[1], 55);
    assert!(packed.get_pixel(0, 0)[2] >= 254);
}

#[test]
fn gltf_groups_all_original_images_and_resolves_its_texture_uris() {
    let (archive, scene) = export_fixture();
    let output = tempfile::tempdir().unwrap();
    export_scene(&archive, &scene, Path::new("triangle.mview"), output.path()).unwrap();
    assert!(output.path().join("triangle.bin").exists());
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(output.path().join("triangle.gltf")).unwrap()).unwrap();
    for name in ["d.png", "s.png", "g.png", "maps/n.png", "unused.png"] {
        assert!(!output.path().join(name).exists());
        assert_eq!(
            fs::read(output.path().join(SOURCE_TEXTURE_DIR).join(name)).unwrap(),
            archive.get(name).unwrap().data
        );
    }
    for image in doc["images"].as_array().unwrap() {
        let uri = image["uri"].as_str().unwrap();
        assert!(
            !uri.contains('/') && !uri.contains('\\'),
            "image must be beside glTF: {uri}"
        );
        assert!(output.path().join(uri).is_file(), "unresolved URI: {uri}");
        if uri.ends_with("n.png") {
            assert_eq!(uri, "n.png");
            assert_eq!(
                fs::read(output.path().join(uri)).unwrap(),
                archive.get("maps/n.png").unwrap().data
            );
        }
    }
}

#[test]
fn flattened_original_texture_names_do_not_overwrite_each_other_or_generated_images() {
    let a = png([160, 128, 255, 255]);
    let b = png([180, 128, 255, 255]);
    let mut bytes = Vec::new();
    append_entry(&mut bytes, "folder-a/n.png", &a);
    append_entry(&mut bytes, "folder-b/N.png", &b);
    let archive = Archive::from_bytes(&bytes).unwrap();
    let mut builder = GltfBuilder::default();
    builder.add_baked_texture("n.png".to_string(), png([0, 0, 0, 255]));
    let mut cache = HashMap::new();
    let first = builder
        .ensure_texture_file(&archive, "folder-a/n.png", &mut cache)
        .unwrap();
    let second = builder
        .ensure_texture_file(&archive, "folder-b/N.png", &mut cache)
        .unwrap();
    assert_eq!(builder.images[first].uri.as_deref(), Some("n_2.png"));
    assert_eq!(builder.images[second].uri.as_deref(), Some("N_3.png"));
    assert_eq!(builder.image_sources[first].data, a);
    assert_eq!(builder.image_sources[second].data, b);
    assert_eq!(
        builder
            .ensure_texture_file(&archive, "folder-a/n.png", &mut cache)
            .unwrap(),
        first
    );
    assert_eq!(builder.images.len(), 3);
    let generated = builder.add_baked_texture("n_2.png".to_string(), png([1, 2, 3, 255]));
    assert_eq!(builder.images[generated].uri.as_deref(), Some("n_2_2.png"));
}

#[test]
fn constant_maps_become_linear_material_factors_without_images() {
    let mut bytes = Vec::new();
    append_entry(&mut bytes, "d.png", &png([0, 0, 0, 255]));
    append_entry(&mut bytes, "s.png", &png([230, 180, 90, 255]));
    append_entry(&mut bytes, "g.png", &png([200, 200, 200, 255]));
    append_entry(&mut bytes, "a.png", &png([60, 0, 0, 255]));
    append_entry(&mut bytes, "n.png", &png([128, 127, 255, 255]));
    let archive = Archive::from_bytes(&bytes).unwrap();
    let material: MaterialDesc = serde_json::from_value(json!({"name":"constant", "albedoTex":"d.png", "reflectivityTex":"s.png", "glossTex":"g.png", "alphaTex":"a.png", "normalTex":"n.png", "ggxSpecular":true})).unwrap();
    let mut builder = GltfBuilder::default();
    builder
        .add_material(&archive, &material, &mut HashMap::new())
        .unwrap();
    let material = serde_json::to_value(&builder.materials[0]).unwrap();
    let pbr = &material["pbrMetallicRoughness"];
    assert!(pbr["baseColorTexture"].is_null());
    assert!(pbr["metallicRoughnessTexture"].is_null());
    assert!(material["normalTexture"].is_null());
    let color = pbr["baseColorFactor"].as_array().unwrap();
    assert!((color[0].as_f64().unwrap() - (230.0_f64 / 255.0).powi(2)).abs() < 1e-5);
    assert!((color[3].as_f64().unwrap() - 60.0 / 255.0).abs() < 1e-6);
    assert!(pbr["metallicFactor"].as_f64().unwrap() > 0.99);
    assert!((pbr["roughnessFactor"].as_f64().unwrap() - 55.0 / 255.0).abs() < 1e-6);
    assert!(builder.images.is_empty());
    assert!(builder.textures.is_empty());
}

#[test]
fn base_color_and_metal_rough_constants_are_optimized_independently() {
    let mut bytes = Vec::new();
    append_entry(&mut bytes, "d.png", &png([0, 0, 0, 255]));
    append_entry(&mut bytes, "s.png", &png([230, 180, 90, 255]));
    let mut gloss = image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 0, 0, 255]));
    gloss.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(gloss)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    append_entry(&mut bytes, "g.png", &encoded.into_inner());
    let archive = Archive::from_bytes(&bytes).unwrap();
    let material: MaterialDesc = serde_json::from_value(json!({"name":"gold", "albedoTex":"d.png", "reflectivityTex":"s.png", "glossTex":"g.png", "ggxSpecular":true})).unwrap();
    let mut builder = GltfBuilder::default();
    builder
        .add_material(&archive, &material, &mut HashMap::new())
        .unwrap();
    let value = serde_json::to_value(&builder.materials[0]).unwrap();
    let pbr = &value["pbrMetallicRoughness"];
    assert!(pbr["baseColorFactor"].is_array());
    assert!(pbr["baseColorTexture"].is_null());
    assert!(pbr["metallicRoughnessTexture"].is_object());
    assert_eq!(builder.images.len(), 1);
}

#[test]
fn disabling_textures_produces_no_source_image_folder() {
    let (archive, mut scene) = export_fixture();
    for material in &mut scene.materials {
        material.albedo_tex.clear();
        material.normal_tex = None;
        material.reflectivity_tex = None;
        material.gloss_tex = None;
    }
    let output = tempfile::tempdir().unwrap();
    export_scene(&archive, &scene, Path::new("plain.mview"), output.path()).unwrap();
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 2);
    assert!(!output.path().join(SOURCE_TEXTURE_DIR).exists());
}

#[test]
fn tangent_frame_is_normalized_and_preserves_handedness() {
    assert_eq!(
        gltf_tangent([0.0, 0.0, 1.0], [2.0, 0.0, 0.2], [0.0, -1.0, 0.0]),
        [1.0, 0.0, 0.0, -1.0]
    );
    let tangent = gltf_tangent([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    assert!((vec3_len([tangent[0], tangent[1], tangent[2]]) - 1.0).abs() < 1e-6);
}
