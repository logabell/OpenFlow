use std::path::Path;

use anyhow::{Context, Result};
use tracing::warn;

pub fn load_whisper(
    model_dir: &Path,
    device: &str,
    compute_type: &str,
    num_threads: Option<i32>,
) -> Result<ct2rs::Whisper> {
    if !model_dir.exists() {
        anyhow::bail!("CT2 model directory not found: {}", model_dir.display());
    }
    if !model_dir.is_dir() {
        anyhow::bail!("CT2 model path is not a directory: {}", model_dir.display());
    }

    let (ct2_device, device_indices) = parse_device(device);
    let ct2_compute_type = parse_compute_type(compute_type);

    let mut config = ct2rs::Config::default();
    config.device = ct2_device;
    config.compute_type = ct2_compute_type;
    if let Some(device_indices) = device_indices {
        config.device_indices = device_indices;
    }

    if let Some(threads) = num_threads
        .filter(|t| *t > 0)
        .and_then(|t| usize::try_from(t).ok())
    {
        config.num_threads_per_replica = threads;
    }

    ct2rs::Whisper::new(model_dir, config).context("init CT2 Whisper")
}

pub fn transcribe(
    recognizer: &mut ct2rs::Whisper,
    samples: &[f32],
    language: Option<&str>,
) -> Result<String> {
    let language = match language {
        Some(lang) if lang.trim().is_empty() => None,
        Some("auto") => None,
        other => other,
    };

    let options = ct2rs::WhisperOptions::default();
    let chunks = recognizer
        .generate(samples, language, false, &options)
        .context("CT2 whisper generate")?;
    Ok(chunks.join("").trim().to_string())
}

fn parse_device(spec: &str) -> (ct2rs::Device, Option<Vec<i32>>) {
    let raw = spec.trim();
    if raw.is_empty() {
        return (ct2rs::Device::CPU, None);
    }

    // Allow `cuda:0` (device + index) syntax.
    let lower = raw.to_ascii_lowercase();
    let mut parts = lower.split(':');
    let dev = parts.next().unwrap_or("cpu");
    let idx = parts.next().and_then(|p| p.parse::<i32>().ok());

    match dev {
        "cuda" | "gpu" => (ct2rs::Device::CUDA, Some(vec![idx.unwrap_or(0).max(0)])),
        "cpu" => (ct2rs::Device::CPU, None),
        other => {
            warn!("Unknown CT2 device '{other}', falling back to cpu");
            (ct2rs::Device::CPU, None)
        }
    }
}

fn parse_compute_type(spec: &str) -> ct2rs::ComputeType {
    let raw = spec.trim();
    if raw.is_empty() {
        return ct2rs::ComputeType::DEFAULT;
    }

    match raw.to_ascii_lowercase().as_str() {
        "default" => ct2rs::ComputeType::DEFAULT,
        "auto" => ct2rs::ComputeType::AUTO,
        "float32" | "fp32" => ct2rs::ComputeType::FLOAT32,
        "float16" | "fp16" | "float" => ct2rs::ComputeType::FLOAT16,
        "bfloat16" | "bf16" => ct2rs::ComputeType::BFLOAT16,
        "int8" => ct2rs::ComputeType::INT8,
        "int16" => ct2rs::ComputeType::INT16,
        "int8_float32" | "int8-float32" | "int8fp32" => ct2rs::ComputeType::INT8_FLOAT32,
        "int8_float16" | "int8-float16" | "int8fp16" => ct2rs::ComputeType::INT8_FLOAT16,
        "int8_bfloat16" | "int8-bfloat16" | "int8bf16" => ct2rs::ComputeType::INT8_BFLOAT16,
        other => {
            warn!("Unknown CT2 compute type '{other}', falling back to default");
            ct2rs::ComputeType::DEFAULT
        }
    }
}
