use std::path::PathBuf;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[cfg(feature = "asr-sherpa")]
use crate::asr::sherpa;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AsrBackend {
    Zipformer,
    Whisper,
    Parakeet,
}

impl Default for AsrBackend {
    fn default() -> Self {
        AsrBackend::Zipformer
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AsrConfig {
    pub backend: AsrBackend,
    pub language: String,
    pub auto_language_detect: bool,
    pub model_dir: Option<PathBuf>,
    pub provider: String,
    pub num_threads: Option<i32>,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            backend: AsrBackend::Zipformer,
            language: "auto".into(),
            auto_language_detect: true,
            model_dir: None,
            provider: "cpu".into(),
            num_threads: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecognitionResult {
    pub text: String,
    pub latency: Duration,
}

pub struct AsrEngine {
    config: AsrConfig,
    buffer: Mutex<Vec<f32>>,
    #[cfg(feature = "asr-sherpa")]
    zipformer: Mutex<Option<sherpa_rs::zipformer::ZipFormer>>,
    #[cfg(feature = "asr-sherpa")]
    whisper: Mutex<Option<sherpa_rs::whisper::WhisperRecognizer>>,
    #[cfg(feature = "asr-sherpa")]
    parakeet: Mutex<Option<sherpa_rs::transducer::TransducerRecognizer>>,
}

impl AsrEngine {
    pub fn new(config: AsrConfig) -> Self {
        Self {
            config,
            buffer: Mutex::new(Vec::new()),
            #[cfg(feature = "asr-sherpa")]
            zipformer: Mutex::new(None),
            #[cfg(feature = "asr-sherpa")]
            whisper: Mutex::new(None),
            #[cfg(feature = "asr-sherpa")]
            parakeet: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &AsrConfig {
        &self.config
    }

    pub fn push_samples(&self, samples: &[f32]) {
        let mut buffer = self.buffer.lock();
        buffer.extend_from_slice(samples);
        Self::truncate_if_needed(&mut buffer);
    }

    pub fn reset(&self) {
        let mut buffer = self.buffer.lock();
        buffer.clear();
    }

    pub fn pending_samples_len(&self) -> usize {
        self.buffer.lock().len()
    }

    pub fn finalize(&self, sample_rate: u32) -> anyhow::Result<Option<RecognitionResult>> {
        let samples = {
            let mut guard = self.buffer.lock();
            if guard.is_empty() {
                return Ok(None);
            }
            std::mem::take(&mut *guard)
        };

        let started = Instant::now();
        #[cfg(feature = "asr-sherpa")]
        let result = self.transcribe_with_sherpa(sample_rate, &samples);

        #[cfg(not(feature = "asr-sherpa"))]
        let result: anyhow::Result<String> = Ok("local asr disabled".into());

        match result {
            Ok(text) => Ok(Some(RecognitionResult {
                text,
                latency: started.elapsed(),
            })),
            Err(error) => {
                warn!("ASR transcription failed: {error:?}");
                Err(error)
            }
        }
    }

    #[cfg(feature = "asr-sherpa")]
    fn transcribe_with_sherpa(&self, sample_rate: u32, samples: &[f32]) -> anyhow::Result<String> {
        if sample_rate != 16_000 {
            anyhow::bail!("ASR requires 16kHz audio (got {sample_rate}Hz)");
        }

        let model_dir = self
            .config
            .model_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ASR model not installed"))?;

        match self.config.backend {
            AsrBackend::Zipformer => {
                let mut guard = self.zipformer.lock();
                if guard.is_none() {
                    info!("Loading Zipformer ASR model from {}", model_dir.display());
                    *guard = Some(sherpa::load_zipformer(
                        model_dir,
                        &self.config.provider,
                        self.config.num_threads,
                    )?);
                    info!("Zipformer ASR model loaded");
                }
                let recognizer = guard
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("zipformer recognizer unavailable"))?;
                Ok(recognizer.decode(sample_rate, samples.to_vec()))
            }
            AsrBackend::Whisper => {
                let mut guard = self.whisper.lock();
                if guard.is_none() {
                    let language = if self.config.auto_language_detect {
                        "auto".to_string()
                    } else {
                        self.config.language.clone()
                    };
                    info!("Loading Whisper ASR model from {}", model_dir.display());
                    *guard = Some(sherpa::load_whisper(
                        model_dir,
                        &language,
                        &self.config.provider,
                        self.config.num_threads,
                    )?);
                    info!("Whisper ASR model loaded");
                }
                let recognizer = guard
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("whisper recognizer unavailable"))?;
                let result = recognizer.transcribe(sample_rate, samples);
                Ok(result.text)
            }
            AsrBackend::Parakeet => {
                let mut guard = self.parakeet.lock();
                if guard.is_none() {
                    info!("Loading Parakeet ASR model from {}", model_dir.display());
                    *guard = Some(sherpa::load_parakeet(
                        model_dir,
                        &self.config.provider,
                        self.config.num_threads,
                    )?);
                    info!("Parakeet ASR model loaded");
                }
                let recognizer = guard
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("parakeet recognizer unavailable"))?;
                Ok(recognizer.transcribe(sample_rate, samples))
            }
        }
    }

    fn truncate_if_needed(buffer: &mut Vec<f32>) {
        const MAX_SAMPLES: usize = 16_000 * 120;
        if buffer.len() > MAX_SAMPLES {
            let overflow = buffer.len() - MAX_SAMPLES;
            buffer.drain(..overflow);
        }
    }
}
