use parking_lot::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VadConfig {
    pub sensitivity: String,
    pub hangover: Duration,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            sensitivity: "medium".into(),
            hangover: Duration::from_millis(400),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VadDecision {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VadBackend {
    Silero,
    Energy,
}

#[derive(Debug, Clone, Copy)]
pub struct VadObservation {
    pub backend: VadBackend,
    pub decision: VadDecision,
    pub score: f32,
    pub threshold: f32,
    pub hangover: Duration,
}

pub struct VoiceActivityDetector {
    config: VadConfig,
    threshold: f32,
    #[cfg(feature = "vad-silero")]
    silero: Option<crate::vad::silero::SileroVad>,
    last_activation: Mutex<Option<Instant>>,
}

impl Default for VoiceActivityDetector {
    fn default() -> Self {
        Self::new(VadConfig::default())
    }
}

impl VoiceActivityDetector {
    pub fn new(config: VadConfig) -> Self {
        // Energy VAD runs *after* preprocessing, which normalizes RMS to ~0.05.
        // That implies mean-square energy around 0.0025 for typical speech.
        // Keep thresholds well below that so the fallback isn't permanently inactive.
        let threshold = match config.sensitivity.as_str() {
            "high" => 0.0006,
            "low" => 0.0018,
            _ => 0.0010,
        };
        #[cfg(feature = "vad-silero")]
        let silero = {
            let speech_threshold = match config.sensitivity.as_str() {
                "high" => 0.45,
                "low" => 0.65,
                _ => 0.55,
            };
            crate::vad::silero::SileroVad::from_env(speech_threshold).ok()
        };
        Self {
            config,
            threshold,
            #[cfg(feature = "vad-silero")]
            silero,
            last_activation: Mutex::new(None),
        }
    }

    pub fn evaluate(&mut self, _frame: &[f32]) -> VadObservation {
        #[cfg(feature = "vad-silero")]
        if let Some(vad) = self.silero.as_mut() {
            let threshold = vad.speech_threshold();
            match vad.ingest(_frame) {
                Ok(prob) => {
                    let speech = prob > threshold;
                    let decision = self.apply_hangover(speech);
                    return VadObservation {
                        backend: VadBackend::Silero,
                        decision,
                        score: prob,
                        threshold,
                        hangover: self.config.hangover,
                    };
                }
                Err(error) => {
                    // If Silero fails at runtime, fall back to energy VAD so dictation
                    // continues to work (and surface the failure via diagnostics).
                    tracing::warn!("Silero VAD failed; falling back to energy: {error:?}");
                    self.silero = None;
                }
            }
        }

        // Simple energy-based heuristic
        let energy = if _frame.is_empty() {
            0.0
        } else {
            _frame.iter().map(|sample| sample * sample).sum::<f32>() / _frame.len() as f32
        };
        let speech = energy > self.threshold;
        let decision = self.apply_hangover(speech);
        VadObservation {
            backend: VadBackend::Energy,
            decision,
            score: energy,
            threshold: self.threshold,
            hangover: self.config.hangover,
        }
    }

    pub fn set_hangover(&mut self, duration: Duration) {
        self.config.hangover = duration;
    }

    pub fn reset(&mut self) {
        *self.last_activation.lock() = None;
        #[cfg(feature = "vad-silero")]
        if let Some(vad) = self.silero.as_mut() {
            vad.reset();
        }
    }

    fn apply_hangover(&self, speech_detected: bool) -> VadDecision {
        if speech_detected {
            let mut guard = self.last_activation.lock();
            *guard = Some(Instant::now());
            return VadDecision::Active;
        }

        let mut guard = self.last_activation.lock();
        if let Some(last) = *guard {
            if last.elapsed() < self.config.hangover {
                return VadDecision::Active;
            }
        }
        *guard = None;
        VadDecision::Inactive
    }
}
