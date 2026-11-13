use std::path::Path;

pub fn load_clean_icon(path: &Path) -> Result<image::RgbaImage, Box<dyn std::error::Error>> {
    let mut image = image::open(path)?.into_rgba8();
    for pixel in image.pixels_mut() {
        if pixel[0] > 245 && pixel[1] > 245 && pixel[2] > 245 {
            pixel[3] = 0;
        }
    }
    Ok(image)
}
