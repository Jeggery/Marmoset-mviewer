use std::fs;
use std::path::PathBuf;

#[path = "../build_support/icon.rs"]
mod icon;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let output_dir = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: cargo run --example generate_icons -- OUTPUT_DIR")?;
    if arguments.next().is_some() {
        return Err("expected exactly one output directory".into());
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("marmoset_logo_red.png");
    let image = icon::load_clean_icon(&source)?;
    fs::create_dir_all(&output_dir)?;
    for size in [16_u32, 32, 64, 128, 256, 512, 1024] {
        let resized =
            image::imageops::resize(&image, size, size, image::imageops::FilterType::Lanczos3);
        resized.save(output_dir.join(format!("mviewer-{size}.png")))?;
    }
    println!("Generated release icons in {}", output_dir.display());
    Ok(())
}
