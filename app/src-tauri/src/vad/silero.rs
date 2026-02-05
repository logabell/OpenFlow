#[cfg(feature = "vad-silero")]
mod silero {
    use anyhow::{anyhow, Context, Result};
    use ort::{
        session::{builder::GraphOptimizationLevel, Session},
        value::Tensor,
    };

    const SAMPLE_RATE: usize = 16_000;
    const FRAME_SIZE: usize = 512;

    pub struct SileroVad {
        session: Session,
        hidden_state: Option<(Vec<usize>, Vec<f32>)>,
        pending: Vec<f32>,
        pending_offset: usize,
        last_probability: f32,
        speech_threshold: f32,
    }

    impl SileroVad {
        pub fn new(model_bytes: &[u8], speech_threshold: f32) -> Result<Self> {
            let session = Session::builder()
                .map_err(|err| anyhow!(err))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|err| anyhow!(err))?
                .commit_from_memory(model_bytes)
                .map_err(|err| anyhow!(err))?;

            Ok(Self {
                session,
                hidden_state: None,
                pending: Vec::with_capacity(FRAME_SIZE * 4),
                pending_offset: 0,
                last_probability: 0.0,
                speech_threshold: speech_threshold.clamp(0.0, 1.0),
            })
        }

        pub fn from_env(speech_threshold: f32) -> Result<Self> {
            let path = std::env::var("SILERO_VAD_MODEL").context("SILERO_VAD_MODEL not set")?;
            let bytes = std::fs::read(path).context("read silero model")?;
            Self::new(&bytes, speech_threshold)
        }

        pub fn reset(&mut self) {
            self.hidden_state = None;
            self.pending.clear();
            self.pending_offset = 0;
            self.last_probability = 0.0;
        }

        pub fn speech_threshold(&self) -> f32 {
            self.speech_threshold
        }

        /// Ingest audio and return the latest speech probability.
        ///
        /// Silero expects contiguous 512-sample windows at 16kHz.
        /// Our capture uses 20ms frames (320 samples), so we buffer across calls
        /// and only run inference on real 512-sample chunks.
        pub fn ingest(&mut self, audio: &[f32]) -> Result<f32> {
            if audio.is_empty() {
                return Ok(self.last_probability);
            }

            self.pending.extend_from_slice(audio);

            while self.pending.len().saturating_sub(self.pending_offset) >= FRAME_SIZE {
                let start = self.pending_offset;
                let end = start + FRAME_SIZE;
                let frame = self.pending[start..end].to_vec();
                self.pending_offset = end;

                let prob = self.run_model(&frame)?;
                self.last_probability = prob;
            }

            // Periodically compact the pending buffer to avoid unbounded growth.
            if self.pending_offset > 0 && self.pending_offset >= FRAME_SIZE * 8 {
                self.pending.drain(..self.pending_offset);
                self.pending_offset = 0;
            }

            Ok(self.last_probability)
        }

        fn run_model(&mut self, frame: &[f32]) -> Result<f32> {
            if frame.len() != FRAME_SIZE {
                return Err(anyhow!(
                    "silero frame size mismatch (got {}, expected {})",
                    frame.len(),
                    FRAME_SIZE
                ));
            }

            let audio_tensor =
                Tensor::from_array(([1usize, FRAME_SIZE], frame.to_vec().into_boxed_slice()))
                    .map_err(|err| anyhow!(err))?;
            let sr_tensor =
                Tensor::from_array(([1usize], vec![SAMPLE_RATE as f32].into_boxed_slice()))
                    .map_err(|err| anyhow!(err))?;

            let outputs = if let Some((state_shape, state_data)) = self.hidden_state.as_ref() {
                let hidden_tensor = Tensor::from_array((
                    state_shape.clone(),
                    state_data.clone().into_boxed_slice(),
                ))
                .map_err(|err| anyhow!(err))?;
                self.session
                    .run(ort::inputs![audio_tensor, sr_tensor, hidden_tensor])
                    .map_err(|err| anyhow!(err))?
            } else {
                self.session
                    .run(ort::inputs![audio_tensor, sr_tensor])
                    .map_err(|err| anyhow!(err))?
            };

            let (_, speech_tensor) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|err| anyhow!(err))?;
            let speech_prob = speech_tensor.first().copied().unwrap_or(0.0);

            let (state_shape, state_tensor) = outputs[1]
                .try_extract_tensor::<f32>()
                .map_err(|err| anyhow!(err))?;
            self.hidden_state = Some((
                state_shape.iter().map(|dim| *dim as usize).collect(),
                state_tensor.to_vec(),
            ));

            Ok(speech_prob)
        }
    }
}

#[cfg(feature = "vad-silero")]
pub use silero::SileroVad;
