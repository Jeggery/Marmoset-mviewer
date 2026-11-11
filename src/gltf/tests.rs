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

#[test]
fn glb_embeds_converted_material_and_authored_tangent_frame() {
    let description = json!({
        "materials": [{"name":"gold", "albedoTex":"d.png", "reflectivityTex":"s.png", "glossTex":"g.png", "ggxSpecular":true}],
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
    append_entry(&mut bytes, "s.png", &png([230, 180, 90, 255]));
    append_entry(&mut bytes, "g.png", &png([200, 200, 200, 255]));
    let archive = Archive::from_bytes(&bytes).unwrap();
    let scene = Scene::from_bytes(&serde_json::to_vec(&description).unwrap()).unwrap();
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
fn tangent_frame_is_normalized_and_preserves_handedness() {
    assert_eq!(
        gltf_tangent([0.0, 0.0, 1.0], [2.0, 0.0, 0.2], [0.0, -1.0, 0.0]),
        [1.0, 0.0, 0.0, -1.0]
    );
    let tangent = gltf_tangent([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    assert!((vec3_len([tangent[0], tangent[1], tangent[2]]) - 1.0).abs() < 1e-6);
}
