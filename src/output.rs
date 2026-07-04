use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Plan output file paths for `n` images.
///
/// - No `--out`: auto-named files in the current directory.
/// - `--out` pointing at an existing directory (or ending in a path separator):
///   auto-named files inside it.
/// - Otherwise `--out` is a file path; with n > 1 an index is inserted before
///   the extension (`img.png` -> `img-1.png`, `img-2.png`, ...).
pub fn plan_paths(out: Option<&Path>, prompt: &str, ext: &str, n: usize) -> Vec<PathBuf> {
    match out {
        None => auto_paths(Path::new("."), prompt, ext, n),
        Some(path) => {
            let is_dir = path.is_dir()
                || path
                    .to_str()
                    .is_some_and(|s| s.ends_with('/') || s.ends_with('\\'));
            if is_dir {
                auto_paths(path, prompt, ext, n)
            } else if n == 1 {
                vec![path.to_path_buf()]
            } else {
                indexed_paths(path, n)
            }
        }
    }
}

fn auto_paths(dir: &Path, prompt: &str, ext: &str, n: usize) -> Vec<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let slug = slugify(prompt, 48);
    let base = if slug.is_empty() {
        format!("imagegen-{stamp}")
    } else {
        format!("{slug}-{stamp}")
    };
    if n == 1 {
        vec![dir.join(format!("{base}.{ext}"))]
    } else {
        (1..=n)
            .map(|i| dir.join(format!("{base}-{i}.{ext}")))
            .collect()
    }
}

fn indexed_paths(path: &Path, n: usize) -> Vec<PathBuf> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png");
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    (1..=n)
        .map(|i| parent.join(format!("{stem}-{i}.{ext}")))
        .collect()
}

/// Turn a prompt into a short filesystem-safe slug.
pub fn slugify(text: &str, max_len: usize) -> String {
    let mut slug = String::with_capacity(max_len);
    let mut last_dash = true; // suppress leading dash
    for c in text.chars() {
        if slug.len() >= max_len {
            break;
        }
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

/// Write image bytes, creating parent directories as needed.
pub fn save_image(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
    }
    std::fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(
            slugify("A cat, sitting on a mat!", 48),
            "a-cat-sitting-on-a-mat"
        );
    }

    #[test]
    fn slugify_truncates() {
        let slug = slugify("word ".repeat(30).as_str(), 20);
        assert!(slug.len() <= 20);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn slugify_unicode_and_symbols() {
        assert_eq!(slugify("héllo wörld 🎨", 48), "h-llo-w-rld");
        assert_eq!(slugify("---", 48), "");
    }

    #[test]
    fn explicit_file_single() {
        let paths = plan_paths(Some(Path::new("out/pic.png")), "x", "png", 1);
        assert_eq!(paths, vec![PathBuf::from("out/pic.png")]);
    }

    #[test]
    fn explicit_file_multi_gets_indexed() {
        let paths = plan_paths(Some(Path::new("pic.jpeg")), "x", "jpeg", 3);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("pic-1.jpeg"),
                PathBuf::from("pic-2.jpeg"),
                PathBuf::from("pic-3.jpeg"),
            ]
        );
    }

    #[test]
    fn trailing_slash_means_directory() {
        let paths = plan_paths(Some(Path::new("outdir/")), "sunset beach", "png", 2);
        assert_eq!(paths.len(), 2);
        for p in &paths {
            assert!(p.starts_with("outdir"));
            let name = p.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("sunset-beach-"));
            assert!(name.ends_with(".png"));
        }
        assert_ne!(paths[0], paths[1]);
    }

    #[test]
    fn default_paths_use_slug_and_extension() {
        let paths = plan_paths(None, "Neon city at night", "webp", 1);
        assert_eq!(paths.len(), 1);
        let name = paths[0].file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("neon-city-at-night-"));
        assert!(name.ends_with(".webp"));
    }
}
