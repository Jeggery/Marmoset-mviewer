use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use byteorder::{LittleEndian, ReadBytesExt};

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub file_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Archive {
    entries: HashMap<String, ArchiveEntry>,
}

/// Bounds archive storage, not image decoding or later export working memory.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    pub max_input_bytes: usize,
    pub max_entry_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_entries: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 * 1024 * 1024,
            max_entry_bytes: 512 * 1024 * 1024,
            max_decoded_bytes: 1024 * 1024 * 1024,
            max_entries: 65_536,
        }
    }
}

impl Archive {
    pub fn from_path(path: &Path) -> Result<Self> {
        Self::from_path_with_limits(path, ArchiveLimits::default())
    }

    pub fn from_path_with_limits(path: &Path, limits: ArchiveLimits) -> Result<Self> {
        let file =
            fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
        ensure!(
            file.metadata()?.len() <= limits.max_input_bytes as u64,
            "archive input exceeds byte limit ({})",
            limits.max_input_bytes
        );
        let mut bytes = Vec::new();
        file.take((limits.max_input_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::from_bytes_with_limits(&bytes, limits)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_bytes_with_limits(bytes, ArchiveLimits::default())
    }

    pub fn from_bytes_with_limits(bytes: &[u8], limits: ArchiveLimits) -> Result<Self> {
        ensure!(
            bytes.len() <= limits.max_input_bytes,
            "archive input exceeds byte limit ({})",
            limits.max_input_bytes
        );
        let mut cursor = std::io::Cursor::new(bytes);
        let mut entries = HashMap::new();
        let mut decoded_bytes = 0usize;

        while (cursor.position() as usize) < bytes.len() {
            let name = read_c_string(&mut cursor).context("failed to read entry name")?;
            let file_type = read_c_string(&mut cursor).context("failed to read entry type")?;

            if name.is_empty()
                && file_type.is_empty()
                && (cursor.position() as usize) == bytes.len()
            {
                break;
            }

            let flags = cursor.read_u32::<LittleEndian>()?;
            let compressed_len = cursor.read_u32::<LittleEndian>()? as usize;
            let uncompressed_len = cursor.read_u32::<LittleEndian>()? as usize;
            ensure!(
                entries.len() < limits.max_entries,
                "archive entry count exceeds limit"
            );
            ensure!(
                !entries.contains_key(&name),
                "duplicate archive entry {name}"
            );
            ensure!(
                uncompressed_len <= limits.max_entry_bytes,
                "archive entry {name} exceeds decoded byte limit ({})",
                limits.max_entry_bytes
            );
            decoded_bytes = decoded_bytes
                .checked_add(uncompressed_len)
                .context("archive decoded length overflow")?;
            ensure!(
                decoded_bytes <= limits.max_decoded_bytes,
                "archive exceeds total decoded byte limit ({})",
                limits.max_decoded_bytes
            );

            let start = cursor.position() as usize;
            let end = start
                .checked_add(compressed_len)
                .context("archive entry length overflow")?;
            if end > bytes.len() {
                bail!("archive entry {} exceeds input size", name);
            }

            cursor.set_position(end as u64);
            let data = if flags & 1 != 0 {
                decompress(&bytes[start..end], uncompressed_len)
                    .with_context(|| format!("failed to decompress {}", name))?
            } else {
                ensure!(
                    compressed_len == uncompressed_len,
                    "uncompressed archive entry {name} has inconsistent lengths"
                );
                bytes[start..end].to_vec()
            };

            entries.insert(
                name.clone(),
                ArchiveEntry {
                    name,
                    file_type,
                    data,
                },
            );
        }

        Ok(Self { entries })
    }

    pub fn get(&self, name: &str) -> Option<&ArchiveEntry> {
        self.entries.get(name)
    }

    pub fn entries(&self) -> Vec<&ArchiveEntry> {
        let mut entries: Vec<_> = self.entries.values().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }
}

fn read_c_string(cursor: &mut std::io::Cursor<&[u8]>) -> Result<String> {
    let bytes = cursor.get_ref();
    let start = cursor.position() as usize;
    let Some(rel_end) = bytes[start..].iter().position(|byte| *byte == 0) else {
        bail!("unterminated c-string");
    };
    let end = start + rel_end;
    let string_bytes = bytes[start..end].to_vec();
    cursor.set_position((end + 1) as u64);
    String::from_utf8(string_bytes).context("invalid utf-8 in c-string")
}

fn decompress(input: &[u8], output_len: usize) -> Result<Vec<u8>> {
    if output_len == 0 {
        ensure!(
            input.is_empty(),
            "non-empty compressed stream with zero decoded length"
        );
        return Ok(Vec::new());
    }
    ensure!(!input.is_empty(), "empty compressed stream");

    // Grow with actual decoded data rather than allocating the advertised size
    // for a tiny or truncated stream. Allocation failures become normal errors.
    let mut output = Vec::new();
    output
        .try_reserve(1)
        .context("failed to allocate decompression output")?;
    let mut table_offsets = [0usize; 4096];
    let mut table_lengths = [0usize; 4096];
    let mut next_code = 256usize;
    let mut write_index = 0usize;
    let mut prev_offset = 0usize;
    let mut prev_length = 1usize;

    output.push(input[0]);
    write_index += 1;

    let mut r = 1usize;
    loop {
        let packed_index = r + (r >> 1);
        if packed_index + 1 >= input.len() {
            break;
        }

        let m = input[packed_index + 1] as usize;
        let n = input[packed_index] as usize;
        let code = if r & 1 == 1 {
            (m << 4) | (n >> 4)
        } else {
            ((m & 15) << 8) | n
        };

        let (entry_offset, entry_length) = if code < next_code {
            if code < 256 {
                ensure_room(write_index, 1, output_len)?;
                output
                    .try_reserve(1)
                    .context("failed to grow decompression output")?;
                output.push(code as u8);
                let current_offset = write_index;
                write_index += 1;
                (current_offset, 1)
            } else {
                let current_offset = write_index;
                let length = table_lengths[code];
                let mut src = table_offsets[code];
                let end = src + length;
                ensure_room(write_index, length, output_len)?;
                output
                    .try_reserve(length)
                    .context("failed to grow decompression output")?;
                while src < end {
                    let byte = *output.get(src).context("invalid dictionary reference")?;
                    output.push(byte);
                    write_index += 1;
                    src += 1;
                }
                (current_offset, length)
            }
        } else if code == next_code {
            let current_offset = write_index;
            let length = prev_length + 1;
            let mut src = prev_offset;
            let end = prev_offset + prev_length;
            ensure_room(write_index, length, output_len)?;
            output
                .try_reserve(length)
                .context("failed to grow decompression output")?;
            while src < end {
                let byte = *output.get(src).context("invalid dictionary reference")?;
                output.push(byte);
                write_index += 1;
                src += 1;
            }
            let byte = *output
                .get(prev_offset)
                .context("invalid dictionary reference")?;
            output.push(byte);
            write_index += 1;
            (current_offset, length)
        } else {
            bail!("invalid compressed dictionary code {code}");
        };

        table_offsets[next_code] = prev_offset;
        table_lengths[next_code] = prev_length + 1;
        next_code += 1;
        prev_offset = entry_offset;
        prev_length = entry_length;
        if next_code >= 4096 {
            next_code = 256;
        }
        r += 1;
    }

    if write_index != output_len {
        bail!(
            "decompression length mismatch: expected {}, got {}",
            output_len,
            write_index
        );
    }

    Ok(output)
}

fn ensure_room(write_index: usize, count: usize, output_len: usize) -> Result<()> {
    if write_index
        .checked_add(count)
        .is_none_or(|end| end > output_len)
    {
        bail!("decompression overflow");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, flags: u32, decoded: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = name.as_bytes().to_vec();
        bytes.extend_from_slice(b"\0bin\0");
        for word in [flags, payload.len() as u32, decoded] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn rejects_zero_length_and_invalid_compressed_streams_without_panicking() {
        for (payload, decoded) in [
            (vec![65], 0),
            (vec![], 1),
            (vec![65], 2),
            (vec![65, 0xf0, 0xff], 2),
            (vec![65, 0x20, 0x04], 1),
        ] {
            assert!(Archive::from_bytes(&entry("data", 1, decoded, &payload)).is_err());
        }
        assert!(
            Archive::from_bytes(&entry("empty", 1, 0, &[]))
                .unwrap()
                .get("empty")
                .unwrap()
                .data
                .is_empty()
        );
        assert_eq!(
            decompress(&[65, 0x20, 0x04, 0x00, 0x01], 4).unwrap(),
            b"ABAB"
        );
        assert_eq!(decompress(&[65, 0x00, 0x10], 3).unwrap(), b"AAA");
        assert!(ensure_room(usize::MAX, 1, usize::MAX).is_err());
    }

    #[test]
    fn enforces_archive_budgets_before_decoding_declared_payloads() {
        let limits = ArchiveLimits {
            max_input_bytes: 1024,
            max_entry_bytes: 3,
            max_decoded_bytes: 5,
            max_entries: 2,
        };
        assert!(
            Archive::from_bytes_with_limits(&entry("large", 1, u32::MAX, &[65]), limits).is_err()
        );
        let mut bytes = entry("a", 0, 3, b"abc");
        bytes.extend(entry("b", 0, 3, b"def"));
        assert!(
            Archive::from_bytes_with_limits(&bytes, limits)
                .unwrap_err()
                .to_string()
                .contains("total decoded")
        );
        let mut bytes = entry("a", 0, 1, b"a");
        bytes.extend(entry("b", 0, 1, b"b"));
        bytes.extend(entry("c", 0, 1, b"c"));
        assert!(
            Archive::from_bytes_with_limits(&bytes, limits)
                .unwrap_err()
                .to_string()
                .contains("entry count")
        );
        let limits = ArchiveLimits {
            max_input_bytes: bytes.len() - 1,
            ..limits
        };
        assert!(Archive::from_bytes_with_limits(&bytes, limits).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), bytes).unwrap();
        assert!(Archive::from_path_with_limits(file.path(), limits).is_err());
    }

    #[test]
    fn truncated_duplicate_and_inconsistent_entries_return_errors() {
        let valid = entry("data", 0, 3, b"abc");
        for end in 1..valid.len() {
            assert!(
                Archive::from_bytes(&valid[..end]).is_err(),
                "accepted prefix {end}"
            );
        }
        assert!(Archive::from_bytes(&entry("data", 0, 4, b"abc")).is_err());
        let mut duplicate = valid.clone();
        duplicate.extend(&valid);
        assert!(
            Archive::from_bytes(&duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }
}
