use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use reqwest::blocking::Client;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 32;

#[derive(Serialize, Deserialize)]
struct Metadata {
    source_url: String,
    file_name: String,
    etag: Option<String>,
    last_modified: Option<String>,
    accessed_at: u64,
}

fn url_hash(url: &reqwest::Url) -> String {
    format!("{:x}", Sha256::digest(url.as_str().as_bytes()))
}

fn readable_entry_path(root: &Path, url: &reqwest::Url, hash_length: usize) -> PathBuf {
    let name = model_name(url);
    let stem = name.strip_suffix(".mview").unwrap_or("scene");
    let stem = &stem[..stem.len().min(32)];
    root.join(format!("{stem}-{}", &url_hash(url)[..hash_length]))
}

fn entry_path(root: &Path, url: &reqwest::Url) -> PathBuf {
    readable_entry_path(root, url, 8)
}

fn read_metadata(directory: &Path) -> Option<Metadata> {
    serde_json::from_slice(&fs::read(directory.join("cache.json")).ok()?).ok()
}

fn owns_entry(root: &Path, directory: &Path, url: &reqwest::Url) -> bool {
    (8..=64)
        .step_by(4)
        .any(|length| directory == readable_entry_path(root, url, length))
}

fn prepare_entry(root: &Path, url: &reqwest::Url) -> Result<PathBuf> {
    // A short hash is readable. On collision, extend it instead of overwriting
    // a different URL's model (or any directory without matching ownership).
    let mut selected = None;
    for length in (8..=64).step_by(4) {
        let candidate = if length == 8 {
            entry_path(root, url)
        } else {
            readable_entry_path(root, url, length)
        };
        match fs::symlink_metadata(&candidate) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                selected = Some(candidate);
                break;
            }
            Err(err) => return Err(err.into()),
            Ok(info) if info.is_dir() && !info.file_type().is_symlink() => {
                let matching =
                    read_metadata(&candidate).is_some_and(|meta| meta.source_url == url.as_str());
                if matching || fs::read_dir(&candidate)?.next().is_none() {
                    selected = Some(candidate);
                    break;
                }
            }
            _ => {}
        }
    }
    let entry = selected.context("could not allocate a unique model cache directory")?;
    fs::create_dir_all(&entry).context("failed to create model cache directory")?;

    Ok(entry)
}

fn model_name(url: &reqwest::Url) -> String {
    let name = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .unwrap_or("scene");
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scene");
    let safe: String = stem
        .chars()
        .take(80)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() { "scene" } else { &safe };
    let upper = safe.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    format!("{}{safe}.mview", if reserved { "_" } else { "" })
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("cache file has no parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|err| err.error)
        .with_context(|| format!("failed to update cache {}", path.display()))?;
    Ok(())
}

pub(super) fn load(
    client: &Client,
    url: &reqwest::Url,
    root: &Path,
    current_scene: Option<&Path>,
) -> Result<PathBuf> {
    let mut url = url.clone();
    url.set_fragment(None);
    let entry = prepare_entry(root, &url)?;
    let model = entry.join(model_name(&url));
    let metadata_path = entry.join("cache.json");
    let previous = fs::read(&metadata_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Metadata>(&bytes).ok())
        .filter(|meta| meta.source_url == url.as_str());
    let cached = model
        .metadata()
        .map(|meta| meta.is_file() && meta.len() > 0)
        .unwrap_or(false);
    let mut request = client.get(url.clone());
    if cached {
        if let Some(meta) = &previous {
            if let Some(etag) = &meta.etag {
                request = request.header(IF_NONE_MATCH, etag);
            }
            if let Some(modified) = &meta.last_modified {
                request = request.header(IF_MODIFIED_SINCE, modified);
            }
        }
    }
    let response = request
        .send()
        .with_context(|| format!("failed to load {url}"))?
        .error_for_status()
        .with_context(|| format!("download failed for {url}"))?;
    let not_modified = response.status() == reqwest::StatusCode::NOT_MODIFIED;
    ensure!(
        !not_modified || cached,
        "server returned 304 without a cached model"
    );
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let last_modified = response
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if !not_modified {
        let mut bytes = Vec::new();
        response
            .take(MAX_CACHE_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("failed to read model download")?;
        ensure!(
            bytes.len() as u64 <= MAX_CACHE_BYTES,
            "model exceeds the 512 MiB cache limit"
        );
        let archive = crate::archive::Archive::from_bytes(&bytes)
            .context("download is not a valid .mview archive")?;
        ensure!(
            archive.get("scene.json").is_some(),
            "download has no scene.json"
        );
        atomic_write(&model, &bytes)?;
    }
    let metadata = Metadata {
        source_url: url.to_string(),
        file_name: model_name(&url),
        etag: if not_modified {
            etag.or_else(|| previous.as_ref().and_then(|m| m.etag.clone()))
        } else {
            etag
        },
        last_modified: if not_modified {
            last_modified.or_else(|| previous.as_ref().and_then(|m| m.last_modified.clone()))
        } else {
            last_modified
        },
        accessed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    atomic_write(&metadata_path, &serde_json::to_vec(&metadata)?)?;
    // Cleanup failure must not prevent a successfully loaded model from opening.
    let _ = prune(
        root,
        &[Some(model.as_path()), current_scene],
        MAX_CACHE_ENTRIES,
        MAX_CACHE_BYTES,
    );
    Ok(model)
}

fn prune(
    root: &Path,
    protected: &[Option<&Path>],
    max_entries: usize,
    max_bytes: u64,
) -> Result<()> {
    let mut entries = Vec::new();
    for item in fs::read_dir(root)? {
        let item = item?;
        // Only consider owned readable directories, never links.
        let kind = item.file_type()?;
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        let directory = item.path();
        let metadata_path = directory.join("cache.json");
        let Some(meta) = fs::read(&metadata_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Metadata>(&bytes).ok())
        else {
            continue;
        };
        let Ok(url) = reqwest::Url::parse(&meta.source_url) else {
            continue;
        };
        if !owns_entry(root, &directory, &url) || meta.file_name != model_name(&url) {
            continue;
        }
        let model = directory.join(&meta.file_name);
        let Ok(info) = fs::symlink_metadata(&model) else {
            continue;
        };
        if !info.is_file() {
            continue;
        }
        entries.push((
            meta.accessed_at,
            info.len(),
            model,
            metadata_path,
            directory,
        ));
    }
    entries.sort_by_key(|entry| entry.0);
    let mut count = entries.len();
    let mut bytes: u64 = entries.iter().map(|entry| entry.1).sum();
    for (_, size, model, metadata_path, directory) in entries {
        if count <= max_entries && bytes <= max_bytes {
            break;
        }
        if protected
            .iter()
            .flatten()
            .any(|path| *path == model.as_path())
        {
            continue;
        }
        if fs::remove_file(&model).is_ok() {
            let _ = fs::remove_file(metadata_path);
            // Non-recursive: any unrelated files or exported models are preserved.
            let _ = fs::remove_dir(directory);
            count -= 1;
            bytes = bytes.saturating_sub(size);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::{Header, Response, Server, StatusCode};

    fn archive(json: &[u8]) -> Vec<u8> {
        let mut data = b"scene.json\0application/json\0".to_vec();
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&(json.len() as u32).to_le_bytes());
        data.extend_from_slice(&(json.len() as u32).to_le_bytes());
        data.extend_from_slice(json);
        data
    }

    #[test]
    fn repeated_url_revalidates_and_updates_one_file_preserving_it_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let url =
            reqwest::Url::parse(&format!("http://{}/lens.mview", server.server_addr())).unwrap();
        let original = archive(b"{}");
        let updated = archive(b"{\"updated\":true}");
        let replies = vec![
            (200, original.clone()),
            (304, Vec::new()),
            (200, updated.clone()),
            (200, b"invalid archive".to_vec()),
            (500, Vec::new()),
        ];
        let worker = std::thread::spawn(move || {
            for (index, (status, body)) in replies.into_iter().enumerate() {
                let request = server
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap()
                    .expect("missing cache request");
                if index == 1 {
                    assert!(request.headers().iter().any(|h| h.field.equiv("If-None-Match") && h.value.as_str() == "\"v1\""));
                }
                request
                    .respond(
                        Response::from_data(body)
                            .with_status_code(StatusCode(status))
                            .with_header(
                                Header::from_bytes(
                                    "ETag",
                                    if index < 2 { "\"v1\"" } else { "\"v2\"" },
                                )
                                .unwrap(),
                            ),
                    )
                    .unwrap();
            }
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let model = load(&client, &url, root.path(), None).unwrap();
        assert_eq!(fs::read(&model).unwrap(), original);
        assert_eq!(load(&client, &url, root.path(), None).unwrap(), model);
        assert_eq!(fs::read(&model).unwrap(), original);
        assert_eq!(load(&client, &url, root.path(), None).unwrap(), model);
        assert_eq!(fs::read(&model).unwrap(), updated);
        assert!(load(&client, &url, root.path(), None).is_err());
        assert_eq!(fs::read(&model).unwrap(), updated);
        assert!(load(&client, &url, root.path(), None).is_err());
        assert_eq!(fs::read(&model).unwrap(), updated);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(model.parent().unwrap()).unwrap().count(), 2);
        worker.join().unwrap();
    }

    #[test]
    fn different_urls_with_same_filename_have_separate_cache_entries() {
        let root = Path::new("cache");
        let a = reqwest::Url::parse("https://example.com/lens.mview?id=1").unwrap();
        let b = reqwest::Url::parse("https://example.com/lens.mview?id=2").unwrap();
        assert_ne!(entry_path(root, &a), entry_path(root, &b));
        assert_eq!(model_name(&a), "lens.mview");
        let directory = entry_path(root, &a);
        let directory = directory.file_name().unwrap().to_str().unwrap();
        assert!(directory.starts_with("lens-"));
        assert_eq!(directory.len(), "lens-".len() + 8);
        assert_eq!(
            model_name(&reqwest::Url::parse("https://example.com/CON.mview").unwrap()),
            "_CON.mview"
        );
    }

    #[test]
    fn short_hash_collision_extends_key_without_overwriting_other_source() {
        let root = tempfile::tempdir().unwrap();
        let a = reqwest::Url::parse("https://example.com/lens.mview?id=a").unwrap();
        let b = reqwest::Url::parse("https://example.com/lens.mview?id=b").unwrap();
        let occupied = entry_path(root.path(), &a);
        fs::create_dir_all(&occupied).unwrap();
        let meta = Metadata {
            source_url: b.to_string(),
            file_name: model_name(&b),
            etag: None,
            last_modified: None,
            accessed_at: 0,
        };
        fs::write(
            occupied.join("cache.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        fs::write(occupied.join("lens.mview"), b"other source").unwrap();
        let selected = prepare_entry(root.path(), &a).unwrap();
        assert_eq!(selected, readable_entry_path(root.path(), &a, 12));
        assert_eq!(prepare_entry(root.path(), &a).unwrap(), selected);
        assert_eq!(
            fs::read(occupied.join("lens.mview")).unwrap(),
            b"other source"
        );
    }

    #[test]
    fn old_full_hash_cache_is_ignored_and_left_untouched() {
        let root = tempfile::tempdir().unwrap();
        let url = reqwest::Url::parse("https://example.com/lens.mview").unwrap();
        let old_directory = root.path().join(url_hash(&url));
        fs::create_dir_all(&old_directory).unwrap();
        let old_model = old_directory.join("lens.mview");
        fs::write(&old_model, b"old model").unwrap();
        let meta = Metadata {
            source_url: url.to_string(),
            file_name: model_name(&url),
            etag: None,
            last_modified: None,
            accessed_at: 0,
        };
        fs::write(
            old_directory.join("cache.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();
        let selected = prepare_entry(root.path(), &url).unwrap();
        assert_eq!(selected, entry_path(root.path(), &url));
        assert!(!selected.join("lens.mview").exists());
        assert!(!selected.join("cache.json").exists());
        prune(root.path(), &[], 0, 0).unwrap();
        assert_eq!(fs::read(old_model).unwrap(), b"old model");
        assert!(old_directory.join("cache.json").exists());
    }

    fn seed(root: &Path, index: u64) -> PathBuf {
        let url = reqwest::Url::parse(&format!("https://example.com/{index}.mview")).unwrap();
        let entry = entry_path(root, &url);
        fs::create_dir_all(&entry).unwrap();
        let model = entry.join(model_name(&url));
        fs::write(&model, [0; 10]).unwrap();
        let metadata = Metadata {
            source_url: url.to_string(),
            file_name: model_name(&url),
            etag: None,
            last_modified: None,
            accessed_at: index,
        };
        fs::write(
            entry.join("cache.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        model
    }

    #[test]
    fn eviction_respects_limits_and_preserves_active_models_and_unrelated_files() {
        let root = tempfile::tempdir().unwrap();
        let active = seed(root.path(), 0);
        let oldest = seed(root.path(), 1);
        let newest = seed(root.path(), 2);
        fs::write(oldest.parent().unwrap().join("user-export.obj"), b"export").unwrap();
        fs::write(root.path().join("notes.txt"), b"notes").unwrap();
        prune(
            root.path(),
            &[Some(active.as_path()), Some(newest.as_path())],
            2,
            20,
        )
        .unwrap();
        assert!(!oldest.exists());
        assert!(oldest.parent().unwrap().join("user-export.obj").exists());
        assert!(active.exists());
        assert!(newest.exists());
        assert!(root.path().join("notes.txt").exists());
        prune(root.path(), &[Some(newest.as_path())], 1, 10).unwrap();
        assert!(!active.exists());
        assert!(newest.exists());
    }
}
