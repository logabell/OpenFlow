use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sherpa_rs::transducer::{TransducerConfig, TransducerRecognizer};
use sherpa_rs::whisper::{WhisperConfig, WhisperRecognizer};

pub fn load_whisper(
    model_dir: &Path,
    language: &str,
    provider: &str,
    num_threads: Option<i32>,
) -> Result<WhisperRecognizer> {
    let config = WhisperConfig {
        encoder: find_component(model_dir, "encoder")?
            .to_string_lossy()
            .into_owned(),
        decoder: find_component(model_dir, "decoder")?
            .to_string_lossy()
            .into_owned(),
        tokens: find_tokens(model_dir)?.to_string_lossy().into_owned(),
        language: language.to_string(),
        provider: Some(provider.to_string()),
        num_threads,
        bpe_vocab: find_vocab(model_dir).map(|path| path.to_string_lossy().into_owned()),
        ..Default::default()
    };
    WhisperRecognizer::new(config).map_err(|err| anyhow::anyhow!("init whisper model: {err}"))
}

pub fn load_parakeet(
    model_dir: &Path,
    provider: &str,
    num_threads: Option<i32>,
) -> Result<TransducerRecognizer> {
    let config = TransducerConfig {
        encoder: find_component(model_dir, "encoder")?
            .to_string_lossy()
            .into_owned(),
        decoder: find_component(model_dir, "decoder")?
            .to_string_lossy()
            .into_owned(),
        joiner: find_component(model_dir, "joiner")?
            .to_string_lossy()
            .into_owned(),
        tokens: find_tokens(model_dir)?.to_string_lossy().into_owned(),
        num_threads: num_threads.unwrap_or(2),
        sample_rate: 16_000,
        feature_dim: 80,
        decoding_method: "greedy_search".to_string(),
        debug: false,
        model_type: "nemo_transducer".to_string(),
        provider: Some(provider.to_string()),
        ..Default::default()
    };
    TransducerRecognizer::new(config).map_err(|err| anyhow::anyhow!("init parakeet model: {err}"))
}

fn find_component(model_dir: &Path, component: &str) -> Result<PathBuf> {
    let direct = model_dir.join(format!("{component}.onnx"));
    if direct.exists() {
        return Ok(direct);
    }

    find_one_level_deep(model_dir, |path| {
        path.extension() == Some(OsStr::new("onnx"))
            && path
                .file_stem()
                .and_then(OsStr::to_str)
                .map(|stem| stem.contains(component))
                .unwrap_or(false)
    })
    .with_context(|| format!("Could not locate {component} ONNX file in {model_dir:?}"))
}

fn find_tokens(model_dir: &Path) -> Result<PathBuf> {
    let default = model_dir.join("tokens.txt");
    if default.exists() {
        return Ok(default);
    }

    find_one_level_deep(model_dir, |path| {
        path.extension() == Some(OsStr::new("txt"))
            && path
                .file_stem()
                .and_then(OsStr::to_str)
                .map(|stem| stem.contains("token"))
                .unwrap_or(false)
    })
    .with_context(|| {
        format!(
            "Could not locate tokens file in {model_dir:?} (expected tokens.txt or *token*.txt)"
        )
    })
}

fn find_vocab(model_dir: &Path) -> Option<PathBuf> {
    find_one_level_deep(model_dir, |path| {
        path.extension() == Some(OsStr::new("txt"))
            && path
                .file_stem()
                .and_then(OsStr::to_str)
                .map(|stem| stem.contains("vocab"))
                .unwrap_or(false)
    })
    .ok()
}

fn find_one_level_deep<F>(dir: &Path, predicate: F) -> Result<PathBuf>
where
    F: Fn(&PathBuf) -> bool,
{
    let mut subdirs = Vec::new();
    for entry in std::fs::read_dir(dir).context("read model directory")? {
        let entry = entry.context("read model directory entry")?;
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
            continue;
        }
        if predicate(&path) {
            return Ok(path);
        }
    }

    for subdir in subdirs {
        for entry in std::fs::read_dir(&subdir).with_context(|| format!("read {subdir:?}"))? {
            let entry = entry.context("read model subdir entry")?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if predicate(&path) {
                return Ok(path);
            }
        }
    }

    anyhow::bail!("no matching file found")
}
