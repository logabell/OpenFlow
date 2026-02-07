use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

const PREPROCESSOR_CONFIG_FILE: &str = "preprocessor_config.json";

// Minimal config expected by ct2rs::Whisper.
#[derive(Debug, Serialize)]
struct PreprocessorConfig {
    chunk_length: usize,
    feature_extractor_type: String,
    feature_size: usize,
    hop_length: usize,
    n_fft: usize,
    n_samples: usize,
    nb_max_frames: usize,
    padding_side: String,
    padding_value: f32,
    processor_class: String,
    return_attention_mask: bool,
    sampling_rate: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    mel_filters: Option<Vec<Vec<f64>>>,
}

pub fn prepare_ct2_model_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Err(anyhow!("model directory not found: {}", dir.display()));
    }
    if !dir.is_dir() {
        return Err(anyhow!("model path is not a directory: {}", dir.display()));
    }

    // Ensure core files exist (and are at the root, not nested).
    ensure_file_at_root(dir, "model.bin")?;
    ensure_file_at_root(dir, "config.json")?;
    ensure_file_at_root(dir, "tokenizer.json")?;
    ensure_any_file_at_root(
        dir,
        &[
            "vocabulary.txt",
            "vocabulary.json",
            "vocab.json",
            "vocab.txt",
        ],
    )
    .context("missing vocabulary file")?;

    ensure_preprocessor_config(dir)?;
    Ok(())
}

fn ensure_preprocessor_config(dir: &Path) -> Result<()> {
    let path = dir.join(PREPROCESSOR_CONFIG_FILE);
    if path.exists() {
        // Sanity check that it's readable JSON.
        let file = fs::File::open(&path).context("open preprocessor_config.json")?;
        let _: serde_json::Value =
            serde_json::from_reader(file).context("parse preprocessor_config.json")?;
        return Ok(());
    }

    if let Some(found) = find_first_with_name_recursive(dir, PREPROCESSOR_CONFIG_FILE) {
        let _ = move_to_root(dir, &found, PREPROCESSOR_CONFIG_FILE);
        if dir.join(PREPROCESSOR_CONFIG_FILE).exists() {
            return Ok(());
        }
    }

    let feature_size = infer_feature_size(dir);
    let config = PreprocessorConfig {
        chunk_length: 30,
        feature_extractor_type: "WhisperFeatureExtractor".into(),
        feature_size,
        hop_length: 160,
        n_fft: 400,
        n_samples: 480_000,
        nb_max_frames: 3000,
        padding_side: "right".into(),
        padding_value: 0.0,
        processor_class: "WhisperProcessor".into(),
        return_attention_mask: false,
        sampling_rate: 16_000,
        mel_filters: None,
    };

    let file = fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
    serde_json::to_writer_pretty(file, &config)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn infer_feature_size(dir: &Path) -> usize {
    let name = dir
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();

    // Whisper large-v3 (and turbo) models use 128 mel bins; most others use 80.
    if name.contains("large") {
        128
    } else {
        80
    }
}

fn ensure_any_file_at_root(dir: &Path, candidates: &[&str]) -> Result<PathBuf> {
    for &name in candidates {
        let direct = dir.join(name);
        if direct.exists() {
            return Ok(direct);
        }
    }

    // Try to find any candidate file and move it to the root.
    for &name in candidates {
        if let Some(found) = find_first_with_name_recursive(dir, name) {
            move_to_root(dir, &found, name)?;
            let direct = dir.join(name);
            if direct.exists() {
                return Ok(direct);
            }
        }
    }

    Err(anyhow!(
        "none of the expected files were found at model root ({}): {}",
        dir.display(),
        candidates.join(", ")
    ))
}

fn ensure_file_at_root(dir: &Path, filename: &str) -> Result<PathBuf> {
    let direct = dir.join(filename);
    if direct.exists() {
        return Ok(direct);
    }

    if let Some(found) = find_first_with_name_recursive(dir, filename) {
        move_to_root(dir, &found, filename)?;
        let direct = dir.join(filename);
        if direct.exists() {
            return Ok(direct);
        }
    }

    Err(anyhow!(
        "required file '{filename}' not found in CT2 model directory {}",
        dir.display()
    ))
}

fn move_to_root(dir: &Path, from: &Path, filename: &str) -> Result<()> {
    let dest = dir.join(filename);
    if dest.exists() {
        // Prefer the root file if it already exists.
        return Ok(());
    }
    fs::rename(from, &dest).with_context(|| {
        format!(
            "move {} from {} to {}",
            filename,
            from.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn find_first_with_name_recursive(dir: &Path, filename: &str) -> Option<PathBuf> {
    let direct = dir.join(filename);
    if direct.exists() {
        return Some(direct);
    }

    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if entry.file_name() == OsStr::new(filename) {
                return Some(path);
            }
        }
    }
    None
}
