use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, ensure};

use super::animated::MeshSkinData;
use crate::mesh::DecodedMesh;
use crate::scene::MeshDesc;
use crate::{archive::Archive, scene::Scene};

#[derive(Clone)]
pub(super) struct EmissiveAtlas {
    pub secondary_uv: bool,
}

pub(super) struct Primitive {
    pub indices: Vec<u32>,
    pub material: Option<usize>,
    pub atlas_uv: bool,
}

fn tile_for_triangle(uv: [[f32; 2]; 3]) -> Result<Option<([f32; 2], bool)>> {
    ensure!(
        uv.iter().flatten().all(|v| v.is_finite()),
        "non-finite emissive UV"
    );
    let tile: [f32; 2] = std::array::from_fn(|axis| {
        uv.iter()
            .map(|v| v[axis])
            .fold(f32::INFINITY, f32::min)
            .floor()
    });
    let constant_integer_v = uv.iter().all(|v| v[1] == tile[1]);
    let seam = uv.iter().any(|v| {
        (0..2).any(|axis| !(axis == 1 && constant_integer_v) && v[axis] > tile[axis] + 1.0)
    });
    Ok((!seam).then_some((tile, constant_integer_v)))
}

// Decide once per source material across ALL selected meshes, before creating
// any images or materials. A repeat seam changes the whole material's strategy.
pub(super) fn repeating_materials(archive: &Archive, scene: &Scene) -> Result<HashSet<String>> {
    let emissive: HashMap<_, _> = scene
        .materials
        .iter()
        .filter(|m| {
            m.extras_tex_coord_ranges
                .as_ref()
                .is_some_and(|r| r.contains_key("emissiveTex"))
        })
        .map(|m| (m.name.as_str(), m.emissive_secondary_uv.unwrap_or(false)))
        .collect();
    let mut repeating = HashSet::new();
    for mesh in &scene.meshes {
        for sub in &mesh.sub_meshes {
            let Some(&secondary) = emissive.get(sub.material.as_str()) else {
                continue;
            };
            if repeating.contains(&sub.material) {
                continue;
            }
            ensure!(
                sub.index_count % 3 == 0,
                "emissive mesh must contain triangles"
            );
            let bytes = &archive
                .get(&mesh.file)
                .context("missing emissive mesh payload")?
                .data;
            let end = sub
                .first_index
                .checked_add(sub.index_count)
                .context("submesh index overflow")?;
            ensure!(end <= mesh.index_count, "submesh indices out of range");
            for first in (sub.first_index..end).step_by(3) {
                if tile_for_triangle(crate::mesh::triangle_uvs(bytes, mesh, first, secondary)?)?
                    .is_none()
                {
                    repeating.insert(sub.material.clone());
                    break;
                }
            }
        }
    }
    Ok(repeating)
}

pub(super) struct PreparedMesh {
    pub decoded: DecodedMesh,
    pub skin: Option<MeshSkinData>,
    pub atlas_uvs: Vec<[f32; 2]>,
    pub primitives: Vec<Primitive>,
}

pub(super) fn prepare_mesh(
    desc: &MeshDesc,
    decoded: &DecodedMesh,
    skin: Option<&MeshSkinData>,
    materials: &HashMap<String, usize>,
    atlases: &HashMap<usize, EmissiveAtlas>,
) -> Result<PreparedMesh> {
    let mut result = PreparedMesh {
        decoded: decoded.clone(),
        skin: skin.cloned(),
        atlas_uvs: vec![[0.0; 2]; decoded.positions.len()],
        primitives: Vec::new(),
    };
    let mut assigned = vec![false; decoded.positions.len()];
    let mut vertices = HashMap::new();
    for sub in &desc.sub_meshes {
        let material = materials.get(&sub.material).copied();
        let atlas = material.and_then(|id| atlases.get(&id));
        let indices = decoded
            .indices
            .get(sub.first_index..sub.first_index + sub.index_count)
            .context("submesh indices out of range")?;
        if let Some(atlas) = atlas {
            let uvs = if atlas.secondary_uv {
                decoded
                    .secondary_texcoords
                    .as_ref()
                    .context("emissive atlas requires secondary UVs")?
            } else {
                &decoded.texcoords
            };
            ensure!(
                indices.len() % 3 == 0,
                "emissive mesh must contain triangles"
            );
            let mut mapped = Vec::with_capacity(indices.len());
            for face in indices.chunks_exact(3) {
                let mut uv = [[0.0; 2]; 3];
                for (corner, index) in face.iter().enumerate() {
                    uv[corner] = *uvs
                        .get(*index as usize)
                        .context("emissive UV index out of range")?;
                }
                let (tile, constant_integer_v) = tile_for_triangle(uv)?
                    .context("emissive repeat seam was not classified before material export")?;
                for (&original, uv) in face.iter().zip(uv) {
                    let local = [
                        uv[0] - tile[0],
                        if constant_integer_v {
                            1.0
                        } else {
                            uv[1] - tile[1]
                        },
                    ];
                    let key = (original, local[0].to_bits(), local[1].to_bits());
                    let index = *vertices.entry(key).or_insert_with(|| {
                        let original = original as usize;
                        let index = if !assigned[original] {
                            assigned[original] = true;
                            original
                        } else {
                            let index = result.decoded.positions.len();
                            duplicate_vertex(&mut result.decoded, decoded, original);
                            if let (Some(dst), Some(src)) = (&mut result.skin, skin) {
                                dst.joints.push(src.joints[original]);
                                dst.weights.push(src.weights[original]);
                                if let (Some(dst), Some(src)) =
                                    (&mut dst.extra_joints, &src.extra_joints)
                                {
                                    dst.push(src[original]);
                                }
                                if let (Some(dst), Some(src)) =
                                    (&mut dst.extra_weights, &src.extra_weights)
                                {
                                    dst.push(src[original]);
                                }
                            }
                            result.atlas_uvs.push([0.0; 2]);
                            index
                        };
                        result.atlas_uvs[index] = local;
                        index as u32
                    });
                    mapped.push(index);
                }
            }
            result.primitives.push(Primitive {
                indices: mapped,
                material,
                atlas_uv: true,
            });
        } else {
            result.primitives.push(Primitive {
                indices: indices.to_vec(),
                material,
                atlas_uv: false,
            });
        }
    }
    Ok(result)
}

fn duplicate_vertex(dst: &mut DecodedMesh, src: &DecodedMesh, index: usize) {
    dst.positions.push(src.positions[index]);
    dst.normals.push(src.normals[index]);
    dst.tangents.push(src.tangents[index]);
    dst.bitangents.push(src.bitangents[index]);
    dst.texcoords.push(src.texcoords[index]);
    if let (Some(dst), Some(src)) = (&mut dst.secondary_texcoords, &src.secondary_texcoords) {
        dst.push(src[index]);
    }
    if let (Some(dst), Some(src)) = (&mut dst.colors, &src.colors) {
        dst.push(src[index]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(uvs: Vec<[f32; 2]>, indices: Vec<u32>) -> (MeshDesc, DecodedMesh) {
        let count = uvs.len();
        let desc = serde_json::from_value(serde_json::json!({
            "name":"uv fixture", "file":"mesh.dat", "vertexCount": count,
            "indexCount": indices.len(), "indexTypeSize":2,"wireCount":0,
            "subMeshes":[{"material":"atlas","firstIndex":0,"indexCount":indices.len()}]
        }))
        .unwrap();
        let mesh = DecodedMesh {
            positions: (0..count).map(|i| [i as f32, 0.0, 0.0]).collect(),
            normals: vec![[0.0, 0.0, 1.0]; count],
            tangents: vec![[1.0, 0.0, 0.0]; count],
            bitangents: vec![[0.0, 1.0, 0.0]; count],
            texcoords: uvs,
            secondary_texcoords: None,
            colors: Some(vec![[0.5; 4]; count]),
            indices,
        };
        (desc, mesh)
    }

    fn atlases() -> HashMap<usize, EmissiveAtlas> {
        HashMap::from([(
            0,
            EmissiveAtlas {
                secondary_uv: false,
            },
        )])
    }

    #[test]
    fn repeated_uvs_are_wrapped_before_the_atlas_transform() {
        let (desc, mesh) = fixture(vec![[2.1, -1.9], [2.8, -1.7], [2.2, -1.1]], vec![0, 1, 2]);
        let prepared = prepare_mesh(
            &desc,
            &mesh,
            None,
            &HashMap::from([("atlas".into(), 0)]),
            &atlases(),
        )
        .unwrap();
        assert!(prepared.primitives[0].atlas_uv);
        for axis in 0..2 {
            let source = mesh.texcoords.iter().map(|uv| uv[axis]).sum::<f32>() / 3.0;
            let actual = prepared.atlas_uvs.iter().map(|uv| uv[axis]).sum::<f32>() / 3.0;
            assert!((actual - (source - source.floor())).abs() < 1e-6);
        }
        assert_eq!(prepared.decoded.positions, mesh.positions);
    }

    #[test]
    fn uv_seam_duplicates_keep_geometry_and_all_skin_weights() {
        let (desc, mesh) = fixture(
            vec![[1.0, 0.0], [0.2, 0.2], [0.4, 0.4], [1.5, 0.2], [1.8, 0.4]],
            vec![0, 1, 2, 0, 3, 4],
        );
        let skin = MeshSkinData {
            joints: vec![[0, 1, 2, 3]; 5],
            weights: vec![[0.2; 4]; 5],
            extra_joints: Some(vec![[4, 5, 0, 0]; 5]),
            extra_weights: Some(vec![[0.1, 0.1, 0.0, 0.0]; 5]),
        };
        let p = prepare_mesh(
            &desc,
            &mesh,
            Some(&skin),
            &HashMap::from([("atlas".into(), 0)]),
            &atlases(),
        )
        .unwrap();
        assert_eq!(p.decoded.positions.len(), 6);
        let duplicate = p.primitives[0].indices[3] as usize;
        assert_eq!(duplicate, 5);
        assert_eq!(p.decoded.positions[duplicate], mesh.positions[0]);
        assert_eq!(p.decoded.texcoords[duplicate], mesh.texcoords[0]);
        assert_eq!(
            p.skin.as_ref().unwrap().extra_weights.as_ref().unwrap()[duplicate],
            skin.extra_weights.as_ref().unwrap()[0]
        );
        assert_eq!(p.atlas_uvs[0], [1.0, 0.0]);
        assert_eq!(p.atlas_uvs[duplicate], [0.0, 0.0]);
    }

    #[test]
    fn repeat_material_keeps_source_primitive_and_uvs_intact() {
        let (desc, mesh) = fixture(
            vec![[0.8, 0.0], [1.2, 0.2], [0.9, 0.4], [0.5, 0.0]],
            vec![0, 1, 2, 0, 2, 3],
        );
        let materials = HashMap::from([("atlas".into(), 0)]);
        assert!(prepare_mesh(&desc, &mesh, None, &materials, &atlases()).is_err());
        // Preflight selects a repeating image for the entire material, so no
        // atlas UV stream or primitive split is needed on any of its meshes.
        let p = prepare_mesh(&desc, &mesh, None, &materials, &HashMap::new()).unwrap();
        assert_eq!(p.primitives.len(), 1);
        assert_eq!(p.primitives[0].indices, mesh.indices);
        assert!(!p.primitives[0].atlas_uv);
        assert_eq!(p.decoded.positions, mesh.positions);
    }

    #[test]
    fn constant_integer_v_uses_the_bottom_of_the_source_tile() {
        let (desc, mesh) = fixture(vec![[2.0, -1.0]; 3], vec![0, 1, 2]);
        let p = prepare_mesh(
            &desc,
            &mesh,
            None,
            &HashMap::from([("atlas".into(), 0)]),
            &atlases(),
        )
        .unwrap();
        assert_eq!(p.atlas_uvs, vec![[0.0, 1.0]; 3]);
        assert!(p.primitives[0].atlas_uv);
    }

    #[test]
    fn secondary_emissive_uvs_do_not_modify_the_primary_uvs() {
        let (desc, mut mesh) = fixture(vec![[0.0, 0.0]; 3], vec![0, 1, 2]);
        mesh.secondary_texcoords = Some(vec![[6.1, -3.9], [6.8, -3.7], [6.2, -3.1]]);
        let mut atlases = atlases();
        atlases.get_mut(&0).unwrap().secondary_uv = true;
        let p = prepare_mesh(
            &desc,
            &mesh,
            None,
            &HashMap::from([("atlas".into(), 0)]),
            &atlases,
        )
        .unwrap();
        assert!((p.atlas_uvs[0][0] - 0.1).abs() < 1e-6);
        assert!((p.atlas_uvs[0][1] - 0.1).abs() < 1e-6);
        assert_eq!(p.decoded.texcoords, mesh.texcoords);
    }
}
