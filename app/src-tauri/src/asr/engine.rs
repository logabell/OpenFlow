use std::path::PathBuf;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[cfg(feature = "asr-ct2")]
use crate::asr::ct2_whisper;
#[cfg(feature = "asr-sherpa")]
use crate::asr::sherpa;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AsrBackend {
    WhisperOnnx,
    WhisperCt2,
    Parakeet,
}

impl Default for AsrBackend {
    fn default() -> Self {
        AsrBackend::Parakeet
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
    pub ct2_device: String,
    pub ct2_compute_type: String,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            backend: AsrBackend::Parakeet,
            language: "auto".into(),
            auto_language_detect: true,
            model_dir: None,
            provider: "cpu".into(),
            num_threads: None,
            ct2_device: "cpu".into(),
            ct2_compute_type: "int8".into(),
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
    whisper: Mutex<Option<sherpa_rs::whisper::WhisperRecognizer>>,
    #[cfg(feature = "asr-sherpa")]
    parakeet: Mutex<Option<sherpa_rs::transducer::TransducerRecognizer>>,
    #[cfg(feature = "asr-ct2")]
    ct2_whisper: Mutex<Option<ct2rs::Whisper>>,
}

impl AsrEngine {
    pub fn new(config: AsrConfig) -> Self {
        Self {
            config,
            buffer: Mutex::new(Vec::new()),
            #[cfg(feature = "asr-sherpa")]
            whisper: Mutex::new(None),
            #[cfg(feature = "asr-sherpa")]
            parakeet: Mutex::new(None),
            #[cfg(feature = "asr-ct2")]
            ct2_whisper: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &AsrConfig {
        &self.config
    }

    pub fn push_samples(&self, samples: &[f32]) -> usize {
        let mut buffer = self.buffer.lock();
        buffer.extend_from_slice(samples);
        Self::truncate_if_needed(&mut buffer)
    }

    pub fn take_samples(&self) -> Vec<f32> {
        let mut buffer = self.buffer.lock();
        std::mem::take(&mut *buffer)
    }

    pub fn reset(&self) {
        let mut buffer = self.buffer.lock();
        buffer.clear();
    }

    pub fn finalize_samples(
        &self,
        sample_rate: u32,
        samples: &[f32],
    ) -> anyhow::Result<Option<RecognitionResult>> {
        if samples.is_empty() {
            return Ok(None);
        }

        let started = Instant::now();
        let result = match self.config.backend {
            AsrBackend::WhisperCt2 => {
                #[cfg(feature = "asr-ct2")]
                {
                    self.transcribe_with_ct2(sample_rate, samples)
                }

                #[cfg(not(feature = "asr-ct2"))]
                {
                    Err(anyhow::anyhow!("CT2 ASR disabled"))
                }
            }
            _ => {
                #[cfg(feature = "asr-sherpa")]
                {
                    self.transcribe_with_sherpa(sample_rate, samples)
                }

                #[cfg(not(feature = "asr-sherpa"))]
                {
                    Err(anyhow::anyhow!("local ASR disabled"))
                }
            }
        };

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

    /// Eagerly load the configured ASR model into memory.
    ///
    /// This is used for startup warmup so the first real transcription does not
    /// pay the model initialization cost.
    pub fn warmup(&self) -> anyhow::Result<()> {
        match self.config.backend {
            AsrBackend::WhisperCt2 => {
                #[cfg(feature = "asr-ct2")]
                {
                    let model_dir = self
                        .config
                        .model_dir
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("ASR model not installed"))?;

                    let mut guard = self.ct2_whisper.lock();
                    if guard.is_none() {
                        info!("Warming CT2 Whisper model from {}", model_dir.display());
                        *guard = Some(ct2_whisper::load_whisper(
                            model_dir,
                            &self.config.ct2_device,
                            &self.config.ct2_compute_type,
                            self.config.num_threads,
                        )?);
                        info!("CT2 Whisper warmup complete");
                    }
                    Ok(())
                }

                #[cfg(not(feature = "asr-ct2"))]
                {
                    anyhow::bail!("CT2 ASR disabled")
                }
            }
            AsrBackend::WhisperOnnx => {
                #[cfg(feature = "asr-sherpa")]
                {
                    let model_dir = self
                        .config
                        .model_dir
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("ASR model not installed"))?;

                    let language = if self.config.auto_language_detect {
                        "auto".to_string()
                    } else {
                        self.config.language.clone()
                    };

                    let mut guard = self.whisper.lock();
                    if guard.is_none() {
                        info!(
                            "Warming Whisper (sherpa) model from {}",
                            model_dir.display()
                        );
                        *guard = Some(sherpa::load_whisper(
                            model_dir,
                            &language,
                            &self.config.provider,
                            self.config.num_threads,
                        )?);
                        info!("Whisper (sherpa) warmup complete");
                    }
                    Ok(())
                }

                #[cfg(not(feature = "asr-sherpa"))]
                {
                    anyhow::bail!("local ASR disabled")
                }
            }
            AsrBackend::Parakeet => {
                #[cfg(feature = "asr-sherpa")]
                {
                    let model_dir = self
                        .config
                        .model_dir
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("ASR model not installed"))?;

                    let mut guard = self.parakeet.lock();
                    if guard.is_none() {
                        info!(
                            "Warming Parakeet (sherpa) model from {}",
                            model_dir.display()
                        );
                        *guard = Some(sherpa::load_parakeet(
                            model_dir,
                            &self.config.provider,
                            self.config.num_threads,
                        )?);
                        info!("Parakeet warmup complete");
                    }
                    Ok(())
                }

                #[cfg(not(feature = "asr-sherpa"))]
                {
                    anyhow::bail!("local ASR disabled")
                }
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
            AsrBackend::WhisperOnnx => {
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
            AsrBackend::WhisperCt2 => anyhow::bail!("CT2 ASR is not handled by sherpa"),
        }
    }

    #[cfg(feature = "asr-ct2")]
    fn transcribe_with_ct2(&self, sample_rate: u32, samples: &[f32]) -> anyhow::Result<String> {
        if sample_rate != 16_000 {
            anyhow::bail!("ASR requires 16kHz audio (got {sample_rate}Hz)");
        }

        let model_dir = self
            .config
            .model_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ASR model not installed"))?;

        let mut guard = self.ct2_whisper.lock();
        if guard.is_none() {
            info!("Loading CT2 Whisper model from {}", model_dir.display());
            *guard = Some(ct2_whisper::load_whisper(
                model_dir,
                &self.config.ct2_device,
                &self.config.ct2_compute_type,
                self.config.num_threads,
            )?);
            info!("CT2 Whisper model loaded");
        }

        let recognizer = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("CT2 whisper recognizer unavailable"))?;

        let language = if self.config.auto_language_detect {
            None
        } else {
            Some(self.config.language.as_str())
        };

        let result = ct2_whisper::transcribe(recognizer, samples, language)?;
        Ok(result)
    }

    fn truncate_if_needed(buffer: &mut Vec<f32>) -> usize {
        const MAX_SAMPLES: usize = 16_000 * 120;
        if buffer.len() > MAX_SAMPLES {
            let overflow = buffer.len() - MAX_SAMPLES;
            buffer.drain(..overflow);
            return overflow;
        }
        0
    }
}
