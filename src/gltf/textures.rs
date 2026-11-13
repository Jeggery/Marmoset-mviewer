use anyhow::{Context, Result};
use image::{Rgba, RgbaImage, imageops::FilterType};

pub(super) struct BakedPbr {
    pub base_color: Vec<u8>,
    pub metallic_roughness: Vec<u8>,
    // Linear values, before PNG quantization. Uniform maps need no image.
    pub base_color_factor: Option<[f32; 4]>,
    pub metallic_roughness_factors: Option<[f32; 2]>, // metallic, roughness
}

pub(super) fn is_neutral_normal(bytes: &[u8]) -> bool {
    let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    if dimensions != Some((1, 1)) {
        return false;
    }
    let Ok(image) = image::load_from_memory(bytes) else {
        return false;
    };
    let pixel = image.to_rgba8().get_pixel(0, 0).0;
    (127..=128).contains(&pixel[0]) && (127..=128).contains(&pixel[1]) && pixel[2] >= 254
}

fn decode(bytes: Option<&[u8]>) -> Result<Option<RgbaImage>> {
    bytes
        .map(|bytes| {
            image::load_from_memory(bytes)
                .map(|image| image.to_rgba8())
                .context("failed to decode material texture")
        })
        .transpose()
}

fn resize(image: Option<RgbaImage>, width: u32, height: u32) -> Option<RgbaImage> {
    image.map(|image| {
        if image.dimensions() == (width, height) {
            image
        } else {
            image::imageops::resize(&image, width, height, FilterType::Triangle)
        }
    })
}

fn encode(image: RgbaImage) -> Result<Vec<u8>> {
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image).write_to(&mut output, image::ImageFormat::Png)?;
    Ok(output.into_inner())
}

fn srgb_byte(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

pub(super) fn convert_emissive(bytes: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut image = decode(bytes)?.unwrap_or_else(|| RgbaImage::from_pixel(1, 1, Rgba([255; 4])));
    // Preserve the complete atlas and its resolution. Only change its encoding.
    for pixel in image.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            *channel = srgb_byte((*channel as f32 / 255.0).powi(2));
        }
        pixel[3] = 255;
    }
    encode(image)
}

// Standard glTF transforms cannot express fract(UV) before an atlas transform
// inside a triangle spanning a repeat seam. A material containing such a seam
// uses one full-resolution repeating image on all its original primitives.
pub(super) fn bake_emissive_repeat(bytes: Option<&[u8]>, range: [f32; 4]) -> Result<Vec<u8>> {
    let image = decode(bytes)?.unwrap_or_else(|| RgbaImage::from_pixel(1, 1, Rgba([255; 4])));
    let [sx, sy, bx, by] = range;
    anyhow::ensure!(range.iter().all(|v| v.is_finite()) && sx > 0.0 && sy > 0.0
        && bx >= 0.0 && by >= 0.0 && bx + sx <= 1.00001 && by + sy <= 1.00001,
        "invalid emissive atlas scaleBias");
    let width = image.width();
    let height = image.height();
    let mut output = RgbaImage::new(width, height);
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        // Original WebGL textures flip Y; decoded image rows and exported UVs do not.
        let u = bx + (x as f32 + 0.5) / width as f32 * sx;
        let v = 1.0 - by - sy + (y as f32 + 0.5) / height as f32 * sy;
        let px = (u * image.width() as f32 - 0.5).clamp(0.0, (image.width() - 1) as f32);
        let py = (v * image.height() as f32 - 0.5).clamp(0.0, (image.height() - 1) as f32);
        let (x0, y0) = (px.floor() as u32, py.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(image.width() - 1), (y0 + 1).min(image.height() - 1));
        let (tx, ty) = (px.fract(), py.fract());
        let rgb: [u8; 3] = std::array::from_fn(|c| {
            let a = image.get_pixel(x0, y0)[c] as f32 * (1.0 - tx) + image.get_pixel(x1, y0)[c] as f32 * tx;
            let b = image.get_pixel(x0, y1)[c] as f32 * (1.0 - tx) + image.get_pixel(x1, y1)[c] as f32 * tx;
            // Match dG(rgb)=rgb*rgb, then encode for glTF's sRGB emissiveTexture.
            srgb_byte(((a * (1.0 - ty) + b * ty) / 255.0).powi(2))
        });
        *pixel = Rgba([rgb[0], rgb[1], rgb[2], 255]);
    }
    encode(output)
}

fn brightness(rgb: [f32; 3]) -> f32 {
    (0.299 * rgb[0] * rgb[0] + 0.587 * rgb[1] * rgb[1] + 0.114 * rgb[2] * rgb[2]).sqrt()
}

// Approximate specular/gloss -> core glTF metal/rough conversion, following
// Khronos' convert-between-workflows reference. Both colors must be LINEAR.
// https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Archived/
// KHR_materials_pbrSpecularGlossiness/examples/convert-between-workflows
fn convert(diffuse: [f32; 3], specular: [f32; 3]) -> ([f32; 3], f32) {
    const F0: f32 = 0.04;
    const EPSILON: f32 = 1e-6;
    let specular_brightness = brightness(specular);
    let remaining = 1.0 - specular.into_iter().fold(0.0_f32, f32::max);
    let metallic = if specular_brightness < F0 {
        0.0
    } else {
        let b = brightness(diffuse) * remaining / (1.0 - F0) + specular_brightness - 2.0 * F0;
        let c = F0 - specular_brightness;
        ((-b + (b * b - 4.0 * F0 * c).max(0.0).sqrt()) / (2.0 * F0)).clamp(0.0, 1.0)
    };
    let diffuse_scale = remaining / (1.0 - F0) / (1.0 - metallic).max(EPSILON);
    let color = std::array::from_fn(|channel| {
        let from_diffuse = diffuse[channel] * diffuse_scale;
        let from_specular = (specular[channel] - F0 * (1.0 - metallic)) / metallic.max(EPSILON);
        let weight = metallic * metallic;
        (from_diffuse * (1.0 - weight) + from_specular * weight).clamp(0.0, 1.0)
    });
    (color, metallic)
}

fn roughness(gloss: f32, ggx: bool) -> f32 {
    if ggx {
        // Marmoset's GGX alpha = (1-gloss)^2; glTF alpha = roughness^2.
        1.0 - gloss
    } else {
        // Older scenes use Blinn-Phong: approximate its lobe width in GGX.
        let exponent = (10.0 / (gloss * 0.968 + 0.03).log2()).powi(2);
        (2.0 / (exponent + 2.0)).powf(0.25)
    }
}

pub(super) fn bake_metallic_roughness(
    albedo: Option<&[u8]>,
    alpha: Option<&[u8]>,
    reflectivity: Option<&[u8]>,
    gloss: Option<&[u8]>,
    ggx: bool,
) -> Result<BakedPbr> {
    let images = [
        decode(albedo)?,
        decode(alpha)?,
        decode(reflectivity)?,
        decode(gloss)?,
    ];
    let width = images
        .iter()
        .flatten()
        .map(|image| image.width())
        .max()
        .unwrap_or(1);
    let height = images
        .iter()
        .flatten()
        .map(|image| image.height())
        .max()
        .unwrap_or(1);
    let [albedo, alpha, reflectivity, gloss] = images.map(|image| resize(image, width, height));
    let mut base_color = RgbaImage::new(width, height);
    let mut metal_rough = RgbaImage::new(width, height);
    let mut first_base = [0.0; 4];
    let mut first_mr = [0.0; 2];
    let mut uniform_base = true;
    let mut uniform_mr = true;
    for y in 0..height {
        for x in 0..width {
            let diffuse_pixel = albedo
                .as_ref()
                .map(|image| *image.get_pixel(x, y))
                .unwrap_or(Rgba([255; 4]));
            // Marmoset dG(rgb) uses rgb*rgb, NOT the piecewise sRGB decoder.
            let diffuse = std::array::from_fn(|c| (diffuse_pixel[c] as f32 / 255.0).powi(2));
            let specular = reflectivity
                .as_ref()
                .map(|image| {
                    std::array::from_fn(|c| (image.get_pixel(x, y)[c] as f32 / 255.0).powi(2))
                })
                .unwrap_or([0.04; 3]);
            let (color, metallic) = convert(diffuse, specular);
            let opacity = alpha
                .as_ref()
                .map(|image| image.get_pixel(x, y)[0])
                .unwrap_or(diffuse_pixel[3]);
            let rough = if let Some(image) = &gloss {
                roughness(image.get_pixel(x, y)[0] as f32 / 255.0, ggx)
            } else if let Some(image) = &reflectivity {
                roughness(image.get_pixel(x, y)[3] as f32 / 255.0, ggx)
            } else {
                1.0
            };
            let linear_base = [color[0], color[1], color[2], opacity as f32 / 255.0];
            let mr = [metallic, rough];
            if x == 0 && y == 0 {
                first_base = linear_base;
                first_mr = mr;
            } else {
                uniform_base &= first_base == linear_base;
                uniform_mr &= first_mr == mr;
            }
            base_color.put_pixel(
                x,
                y,
                Rgba([
                    srgb_byte(color[0]),
                    srgb_byte(color[1]),
                    srgb_byte(color[2]),
                    opacity,
                ]),
            );
            // glTF: G=linear perceptual roughness, B=linear metallic.
            metal_rough.put_pixel(
                x,
                y,
                Rgba([
                    255,
                    (rough * 255.0).round() as u8,
                    (metallic * 255.0).round() as u8,
                    255,
                ]),
            );
        }
    }
    Ok(BakedPbr {
        base_color: encode(base_color)?,
        metallic_roughness: encode(metal_rough)?,
        base_color_factor: uniform_base.then_some(first_base),
        metallic_roughness_factors: uniform_mr.then_some(first_mr),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixel(value: [u8; 4]) -> Vec<u8> {
        encode(RgbaImage::from_pixel(1, 1, Rgba(value))).unwrap()
    }

    #[test]
    fn dark_diffuse_colored_specular_becomes_colored_metal() {
        let d = pixel([0, 0, 0, 255]);
        let s = pixel([230, 180, 90, 255]);
        let g = pixel([200, 200, 200, 255]);
        let baked = bake_metallic_roughness(Some(&d), None, Some(&s), Some(&g), true).unwrap();
        let base = image::load_from_memory(&baked.base_color)
            .unwrap()
            .to_rgba8();
        let mr = image::load_from_memory(&baked.metallic_roughness)
            .unwrap()
            .to_rgba8();
        assert!(mr.get_pixel(0, 0)[2] >= 254);
        assert_eq!(mr.get_pixel(0, 0)[1], 55);
        let p = base.get_pixel(0, 0);
        assert!(p[0] > 220 && p[0] > p[1] && p[1] > p[2]);
    }

    #[test]
    fn dielectric_is_not_fully_metallic_and_alpha_uses_red() {
        let d = pixel([128, 128, 128, 255]);
        let s = pixel([51, 51, 51, 255]); // (51/255)^2 = 0.04 dielectric F0
        let a = pixel([60, 200, 240, 255]);
        let baked = bake_metallic_roughness(Some(&d), Some(&a), Some(&s), None, true).unwrap();
        let base = image::load_from_memory(&baked.base_color)
            .unwrap()
            .to_rgba8();
        let mr = image::load_from_memory(&baked.metallic_roughness)
            .unwrap()
            .to_rgba8();
        assert_eq!(mr.get_pixel(0, 0)[2], 0);
        assert_eq!(base.get_pixel(0, 0)[3], 60);
        assert!((base.get_pixel(0, 0)[0] as i32 - 137).abs() <= 1);
    }

    #[test]
    fn conversion_recovers_known_metal_rough_parameters() {
        let original = [0.7, 0.4, 0.1];
        // Khronos' perceived-brightness solve is approximate for colored,
        // partially metallic pixels; these estimates come from its reference.
        for (metallic, expected) in [
            (0.0_f32, 0.0),
            (0.25, 0.24710127),
            (0.75, 0.74962886),
            (1.0, 1.0),
        ] {
            let specular = original.map(|c| 0.04 * (1.0 - metallic) + c * metallic);
            let remaining = 1.0 - specular.into_iter().fold(0.0_f32, f32::max);
            let diffuse = original.map(|c| c * 0.96 * (1.0 - metallic) / remaining);
            let (converted, m) = convert(diffuse, specular);
            assert!((m - expected).abs() < 1e-5);
            for channel in 0..3 {
                assert!((converted[channel] - original[channel]).abs() < 0.003);
            }
        }
    }

    #[test]
    fn missing_maps_default_to_dielectric_and_legacy_gloss_uses_lobe_conversion() {
        let baked = bake_metallic_roughness(None, None, None, None, false).unwrap();
        let mr = image::load_from_memory(&baked.metallic_roughness)
            .unwrap()
            .to_rgba8();
        assert_eq!(*mr.get_pixel(0, 0), Rgba([255, 255, 0, 255]));
        assert!(roughness(0.5, false) < 0.4);
        assert!(roughness(0.9, false) < roughness(0.1, false));
        assert!((roughness(0.5, true) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn higher_resolution_specular_is_not_lost_with_a_constant_diffuse_map() {
        let d = pixel([0, 0, 0, 255]);
        let mut specular = RgbaImage::new(2, 1);
        specular.put_pixel(0, 0, Rgba([230, 180, 90, 255]));
        specular.put_pixel(1, 0, Rgba([30, 30, 30, 255]));
        let s = encode(specular).unwrap();
        let baked = bake_metallic_roughness(Some(&d), None, Some(&s), None, true).unwrap();
        let base = image::load_from_memory(&baked.base_color)
            .unwrap()
            .to_rgba8();
        let mr = image::load_from_memory(&baked.metallic_roughness)
            .unwrap()
            .to_rgba8();
        assert_eq!(base.dimensions(), (2, 1));
        assert!(mr.get_pixel(0, 0)[2] >= 254);
        assert_eq!(mr.get_pixel(1, 0)[2], 0);
        assert!(base.get_pixel(0, 0)[0] > 220);
    }
}
