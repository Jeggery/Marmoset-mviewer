use super::*;

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
