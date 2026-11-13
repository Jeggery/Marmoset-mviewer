#[cfg(target_os = "windows")]
use std::fs::File;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[cfg(target_os = "windows")]
#[path = "build_support/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=marmoset_logo_red.png");

    #[cfg(target_os = "windows")]
    {
        if let Err(err) = embed_windows_icon() {
            panic!("failed to embed Windows icon: {err}");
        }
    }
}

#[cfg(target_os = "windows")]
fn embed_windows_icon() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let source_path = manifest_dir.join("marmoset_logo_red.png");
    let output_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let icon_path = output_dir.join("mviewer.ico");

    let image = icon::load_clean_icon(&source_path)?;
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let resized =
            image::imageops::resize(&image, size, size, image::imageops::FilterType::Lanczos3);
        let icon_image = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
        icon_dir.add_entry(ico::IconDirEntry::encode(&icon_image)?);
    }

    let mut file = File::create(&icon_path)?;
    icon_dir.write(&mut file)?;

    winresource::WindowsResource::new()
        .set("ProductName", "Marmoset-mviewer")
        .set(
            "FileDescription",
            "Marmoset .mview viewer and exporter - Jeggery fork",
        )
        .set_icon(icon_path.to_string_lossy().as_ref())
        .compile()?;

    Ok(())
}
