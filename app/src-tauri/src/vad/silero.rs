#[cfg(feature = "vad-silero")]
mod silero {
    use anyhow::{anyhow, Context, Result};
    use std::ffi::CString;

    use sherpa_rs_sys as sys;

    const SAMPLE_RATE: i32 = 16_000;
    const WINDOW_SIZE: i32 = 512;
    const BUFFER_SIZE_SECONDS: f32 = 30.0;

    pub struct SileroVad {
        vad: *const sys::SherpaOnnxVoiceActivityDetector,
        last_score: f32,
        speech_threshold: f32,
    }

    impl SileroVad {
        pub fn new(model_path: &str, speech_threshold: f32) -> Result<Self> {
            let provider = std::env::var("SHERPA_PROVIDER").unwrap_or_else(|_| "cpu".into());
            let num_threads = std::env::var("SHERPA_THREADS")
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(1);

            Self::new_with_runtime(model_path, speech_threshold, &provider, num_threads, false)
        }

        pub fn from_env(speech_threshold: f32) -> Result<Self> {
            let model_path =
                std::env::var("SILERO_VAD_MODEL").context("SILERO_VAD_MODEL not set")?;
            Self::new(&model_path, speech_threshold)
        }

        fn new_with_runtime(
            model_path: &str,
            speech_threshold: f32,
            provider: &str,
            num_threads: i32,
            debug: bool,
        ) -> Result<Self> {
            // sherpa-onnx validates threshold to be >= 0.01 and < 1.0.
            let speech_threshold = speech_threshold.clamp(0.01, 0.99);

            let model_c = CString::new(model_path).context("silero model path contains NUL")?;
            let provider_c = CString::new(provider).context("provider contains NUL")?;

            let silero_config = sys::SherpaOnnxSileroVadModelConfig {
                model: model_c.as_ptr(),
                threshold: speech_threshold,
                // Keep these low; OpenFlow applies its own hangover in VoiceActivityDetector.
                min_silence_duration: 0.1,
                min_speech_duration: 0.15,
                window_size: WINDOW_SIZE,
                max_speech_duration: 20.0,
            };

            let vad_config = sys::SherpaOnnxVadModelConfig {
                silero_vad: silero_config,
                sample_rate: SAMPLE_RATE,
                num_threads,
                provider: provider_c.as_ptr(),
                debug: if debug { 1 } else { 0 },
                // ten_vad is unused when silero_vad.model is set.
                ten_vad: unsafe { std::mem::zeroed::<sys::SherpaOnnxTenVadModelConfig>() },
            };

            let vad = unsafe {
                sys::SherpaOnnxCreateVoiceActivityDetector(&vad_config, BUFFER_SIZE_SECONDS)
            };
            if vad.is_null() {
                return Err(anyhow!(
                    "failed to create SherpaOnnxVoiceActivityDetector (silero model: {})",
                    model_path
                ));
            }

            Ok(Self {
                vad,
                last_score: 0.0,
                speech_threshold,
            })
        }

        pub fn reset(&mut self) {
            self.last_score = 0.0;
            unsafe {
                sys::SherpaOnnxVoiceActivityDetectorReset(self.vad);
            }
        }

        pub fn speech_threshold(&self) -> f32 {
            self.speech_threshold
        }

        /// Ingest audio and return a 0..1 speech score.
        ///
        /// sherpa-onnx VAD is stateful and returns a detected/silent decision.
        /// We expose it as `1.0` / `0.0` to fit OpenFlow's diagnostics interface.
        pub fn ingest(&mut self, audio: &[f32]) -> Result<f32> {
            if audio.is_empty() {
                return Ok(self.last_score);
            }

            let n = i32::try_from(audio.len()).map_err(|_| anyhow!("audio frame too large"))?;
            unsafe {
                sys::SherpaOnnxVoiceActivityDetectorAcceptWaveform(self.vad, audio.as_ptr(), n);
            }

            let detected = unsafe { sys::SherpaOnnxVoiceActivityDetectorDetected(self.vad) } != 0;
            let score = if detected { 1.0 } else { 0.0 };
            self.last_score = score;
            Ok(score)
        }
    }

    impl Drop for SileroVad {
        fn drop(&mut self) {
            unsafe {
                sys::SherpaOnnxDestroyVoiceActivityDetector(self.vad);
            }
        }
    }

    unsafe impl Send for SileroVad {}
    unsafe impl Sync for SileroVad {}
}

#[cfg(feature = "vad-silero")]
pub use silero::SileroVad;
