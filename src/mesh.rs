use std::io::Cursor;

use anyhow::{Context, Result, bail};
use byteorder::{LittleEndian, ReadBytesExt};

use crate::scene::MeshDesc;

#[derive(Debug, Clone)]
pub struct DecodedMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub bitangents: Vec<[f32; 3]>,
    pub texcoords: Vec<[f32; 2]>,
    pub secondary_texcoords: Option<Vec<[f32; 2]>>,
    pub colors: Option<Vec<[f32; 4]>>,
    pub indices: Vec<u32>,
}

// Read just the UVs needed by the material preflight, without decoding/copying
// positions, tangent frames, or skin streams for every mesh a second time.
pub(crate) fn triangle_uvs(
    bytes: &[u8],
    mesh: &MeshDesc,
    first_index: usize,
    secondary: bool,
) -> Result<[[f32; 2]; 3]> {
    anyhow::ensure!(
        first_index
            .checked_add(2)
            .is_some_and(|end| end < mesh.index_count),
        "triangle index out of range"
    );
    anyhow::ensure!(
        !secondary || mesh.secondary_tex_coord.unwrap_or(0) > 0,
        "emissive atlas requires secondary UVs"
    );
    let index_size = mesh.index_type_size;
    anyhow::ensure!(
        matches!(index_size, 2 | 4),
        "unsupported index size {index_size}"
    );
    let vertices = mesh
        .index_count
        .checked_add(mesh.wire_count)
        .and_then(|count| count.checked_mul(index_size))
        .context("vertex data offset overflow")?;
    let stride = 32
        + usize::from(mesh.secondary_tex_coord.unwrap_or(0) > 0) * 8
        + usize::from(mesh.vertex_color.unwrap_or(0) > 0) * 4;
    let read = |offset: usize, size: usize| -> Result<&[u8]> {
        bytes
            .get(offset..offset.checked_add(size).context("mesh offset overflow")?)
            .context("mesh payload too short")
    };
    let mut result = [[0.0; 2]; 3];
    for (corner, uv) in result.iter_mut().enumerate() {
        let offset = (first_index + corner)
            .checked_mul(index_size)
            .context("index offset overflow")?;
        let index = if index_size == 2 {
            u16::from_le_bytes(read(offset, 2)?.try_into()?) as usize
        } else {
            u32::from_le_bytes(read(offset, 4)?.try_into()?) as usize
        };
        anyhow::ensure!(index < mesh.vertex_count, "vertex index out of range");
        let offset = index
            .checked_mul(stride)
            .and_then(|n| n.checked_add(vertices))
            .and_then(|n| n.checked_add(12 + usize::from(secondary) * 8))
            .context("UV offset overflow")?;
        let data = read(offset, 8)?;
        *uv = [
            f32::from_le_bytes(data[..4].try_into()?),
            -f32::from_le_bytes(data[4..].try_into()?),
        ];
    }
    Ok(result)
}

pub fn decode_mesh(bytes: &[u8], mesh: &MeshDesc) -> Result<DecodedMesh> {
    let mut cursor = Cursor::new(bytes);
    let mut all_indices = Vec::with_capacity(mesh.index_count);

    for _ in 0..mesh.index_count {
        let index = match mesh.index_type_size {
            2 => cursor.read_u16::<LittleEndian>()? as u32,
            4 => cursor.read_u32::<LittleEndian>()?,
            other => bail!("unsupported index size {}", other),
        };
        all_indices.push(index);
    }

    let wire_skip = mesh
        .wire_count
        .checked_mul(mesh.index_type_size)
        .context("wire count overflow")?;
    cursor.set_position(cursor.position() + wire_skip as u64);

    let has_secondary_uv = mesh.secondary_tex_coord.unwrap_or(0) > 0;
    let has_vertex_color = mesh.vertex_color.unwrap_or(0) > 0;

    let mut positions = Vec::with_capacity(mesh.vertex_count);
    let mut normals = Vec::with_capacity(mesh.vertex_count);
    let mut tangents = Vec::with_capacity(mesh.vertex_count);
    let mut bitangents = Vec::with_capacity(mesh.vertex_count);
    let mut texcoords = Vec::with_capacity(mesh.vertex_count);
    let mut secondary_texcoords = if has_secondary_uv {
        Some(Vec::with_capacity(mesh.vertex_count))
    } else {
        None
    };
    let mut colors = if has_vertex_color {
        Some(Vec::with_capacity(mesh.vertex_count))
    } else {
        None
    };

    for _ in 0..mesh.vertex_count {
        positions.push([
            cursor.read_f32::<LittleEndian>()?,
            cursor.read_f32::<LittleEndian>()?,
            cursor.read_f32::<LittleEndian>()?,
        ]);

        texcoords.push([
            cursor.read_f32::<LittleEndian>()?,
            -cursor.read_f32::<LittleEndian>()?,
        ]);

        if has_secondary_uv {
            let uv = [
                cursor.read_f32::<LittleEndian>()?,
                -cursor.read_f32::<LittleEndian>()?,
            ];
            if let Some(secondary) = &mut secondary_texcoords {
                secondary.push(uv);
            }
        }

        tangents.push(unpack_unit_vector(
            cursor.read_u16::<LittleEndian>()?,
            cursor.read_u16::<LittleEndian>()?,
        ));
        bitangents.push(unpack_unit_vector(
            cursor.read_u16::<LittleEndian>()?,
            cursor.read_u16::<LittleEndian>()?,
        ));
        normals.push(unpack_unit_vector(
            cursor.read_u16::<LittleEndian>()?,
            cursor.read_u16::<LittleEndian>()?,
        ));

        if let Some(vertex_colors) = &mut colors {
            let r = cursor.read_u8()? as f32 / 255.0;
            let g = cursor.read_u8()? as f32 / 255.0;
            let b = cursor.read_u8()? as f32 / 255.0;
            let a = cursor.read_u8()? as f32 / 255.0;
            vertex_colors.push([r, g, b, a]);
        }
    }

    Ok(DecodedMesh {
        positions,
        normals,
        tangents,
        bitangents,
        texcoords,
        secondary_texcoords,
        colors,
        indices: all_indices,
    })
}

fn unpack_unit_vector(raw_x: u16, raw_y: u16) -> [f32; 3] {
    let mut y = raw_y as i32;
    let z_negative = y >= 32768;
    if z_negative {
        y -= 32768;
    }

    let x = raw_x as f32 / 32767.4 * 2.0 - 1.0;
    let y = y as f32 / 32767.4 * 2.0 - 1.0;
    let z_sq = (1.0 - (x * x + y * y)).max(0.0);
    let mut z = z_sq.sqrt();
    if z_negative {
        z = -z;
    }

    [x, y, z]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_preflight_matches_decoder_with_wire_indices_secondary_uv_and_vertex_color() {
        let desc: MeshDesc = serde_json::from_value(serde_json::json!({
            "name":"layout", "file":"mesh.dat", "vertexCount":3,"indexCount":3,
            "wireCount":2,"indexTypeSize":4,"secondaryTexCoord":1,"vertexColor":1,"subMeshes":[]
        }))
        .unwrap();
        let mut bytes = Vec::new();
        for value in [2_u32, 0, 1, 0, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for i in 0..3 {
            for value in [
                i as f32,
                1.0,
                2.0,
                0.1 + i as f32,
                0.2 + i as f32,
                3.1 + i as f32,
                3.2 + i as f32,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&[0; 12]);
            bytes.extend_from_slice(&[10, 20, 30, 255]);
        }
        let decoded = decode_mesh(&bytes, &desc).unwrap();
        assert_eq!(
            triangle_uvs(&bytes, &desc, 0, false).unwrap(),
            [
                decoded.texcoords[2],
                decoded.texcoords[0],
                decoded.texcoords[1]
            ]
        );
        let secondary = decoded.secondary_texcoords.unwrap();
        assert_eq!(
            triangle_uvs(&bytes, &desc, 0, true).unwrap(),
            [secondary[2], secondary[0], secondary[1]]
        );
        assert!(triangle_uvs(&bytes[..1], &desc, 0, false).is_err());
        assert!(triangle_uvs(&bytes, &desc, 2, false).is_err());
        bytes[..4].copy_from_slice(&99_u32.to_le_bytes());
        assert!(triangle_uvs(&bytes, &desc, 0, false).is_err());
    }
}
