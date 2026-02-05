use serde::{Deserialize, Serialize};
use tracing::warn;

#[cfg(feature = "webrtc-apm")]
use webrtc_audio_processing::{
    Config as WebRtcConfig, GainControl, GainControlMode, InitializationConfig, NoiseSuppression,
    NoiseSuppressionLevel, Processor as WebRtcProcessor, NUM_SAMPLES_PER_FRAME,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioProcessingMode {
    Standard,
    Enhanced,
}

impl Default for AudioProcessingMode {
    fn default() -> Self {
        AudioProcessingMode::Standard
    }
}

pub struct AudioPreprocessor {
    apm: ApmStage,
    denoiser: Option<EnhancedStage>,
    preferred: AudioProcessingMode,
    performance_override: bool,
}

impl AudioPreprocessor {
    pub fn new(mode: AudioProcessingMode) -> Self {
        Self {
            apm: ApmStage::new(),
            denoiser: match mode {
                AudioProcessingMode::Enhanced => Some(EnhancedStage::new()),
                AudioProcessingMode::Standard => None,
            },
            preferred: mode,
            performance_override: false,
        }
    }

    pub fn process(&mut self, frame: &mut [f32]) {
        if frame.is_empty() {
            return;
        }

        self.apm.process(frame);

        if self.performance_override {
            return;
        }

        if matches!(self.preferred, AudioProcessingMode::Enhanced) {
            if self.denoiser.is_none() {
                self.denoiser = Some(EnhancedStage::new());
            }

            if let Some(denoiser) = self.denoiser.as_mut() {
                denoiser.process(frame);
            }
        } else {
            self.denoiser = None;
        }
    }

    pub fn set_preferred_mode(&mut self, mode: AudioProcessingMode) {
        self.preferred = mode;
        if !matches!(mode, AudioProcessingMode::Enhanced) {
            self.denoiser = None;
        }
    }

    pub fn preferred_mode(&self) -> AudioProcessingMode {
        self.preferred
    }

    pub fn effective_mode(&self) -> AudioProcessingMode {
        if self.performance_override {
            AudioProcessingMode::Standard
        } else if matches!(self.preferred, AudioProcessingMode::Enhanced) {
            AudioProcessingMode::Enhanced
        } else {
            AudioProcessingMode::Standard
        }
    }

    pub fn set_performance_override(&mut self, enabled: bool) {
        self.performance_override = enabled;
        if enabled {
            self.denoiser = None;
        }
    }
}

enum ApmStage {
    #[cfg(feature = "webrtc-apm")]
    WebRtc(WebRtcApm),
    Stub(BaselineProcessor),
}

impl ApmStage {
    fn new() -> Self {
        #[cfg(feature = "webrtc-apm")]
        {
            match WebRtcApm::new() {
                Ok(apm) => return ApmStage::WebRtc(apm),
                Err(error) => warn!("Falling back to baseline audio preprocessing: {error}"),
            }
        }
        ApmStage::Stub(BaselineProcessor::new())
    }

    fn process(&mut self, frame: &mut [f32]) {
        match self {
            #[cfg(feature = "webrtc-apm")]
            ApmStage::WebRtc(apm) => apm.process(frame),
            ApmStage::Stub(stub) => stub.process(frame),
        }
    }
}

#[cfg(feature = "webrtc-apm")]
struct WebRtcApm {
    processor: WebRtcProcessor,
    frame_len: usize,
    channels: usize,
    scratch: Vec<f32>,
}

#[cfg(feature = "webrtc-apm")]
impl WebRtcApm {
    fn new() -> Result<Self, webrtc_audio_processing::Error> {
        let mut init = InitializationConfig::default();
        init.num_capture_channels = 1;
        init.num_render_channels = 0;

        let processor = WebRtcProcessor::new(&init)?;

        let mut instance = Self {
            processor,
            frame_len: NUM_SAMPLES_PER_FRAME as usize,
            channels: init.num_capture_channels as usize,
            scratch: vec![0.0; NUM_SAMPLES_PER_FRAME as usize * init.num_capture_channels as usize],
        };
        instance.configure();
        Ok(instance)
    }

    fn configure(&mut self) {
        let config = WebRtcConfig {
            echo_cancellation: None,
            gain_control: Some(GainControl {
                mode: GainControlMode::AdaptiveDigital,
                target_level_dbfs: 3,
                compression_gain_db: 9,
                enable_limiter: true,
            }),
            noise_suppression: Some(NoiseSuppression {
                suppression_level: NoiseSuppressionLevel::High,
            }),
            voice_detection: None,
            enable_transient_suppressor: true,
            enable_high_pass_filter: true,
        };
        self.processor.set_config(config);
    }

    fn process(&mut self, frame: &mut [f32]) {
        let chunk_size = self.frame_len * self.channels;
        if chunk_size == 0 {
            return;
        }

        for chunk in frame.chunks_mut(chunk_size) {
            if chunk.len() != chunk_size {
                // Pad remainder with zeros before processing.
                self.scratch.fill(0.0);
                self.scratch[..chunk.len()].copy_from_slice(chunk);
                if let Err(error) = self.processor.process_capture_frame(&mut self.scratch) {
                    warn!("webrtc-audio-processing partial frame failed: {error}");
                    continue;
                }
                chunk.copy_from_slice(&self.scratch[..chunk.len()]);
            } else if let Err(error) = self.processor.process_capture_frame(chunk) {
                warn!("webrtc-audio-processing failed: {error}");
            }
        }
    }
}

#[derive(Debug)]
struct BaselineProcessor {
    target_rms: f32,
    smoothing: f32,
    last_gain: f32,
}

impl BaselineProcessor {
    fn new() -> Self {
        Self {
            target_rms: 0.05,
            smoothing: 0.85,
            last_gain: 1.0,
        }
    }

    fn process(&mut self, frame: &mut [f32]) {
        let mean = frame.iter().copied().sum::<f32>() / frame.len() as f32;
        for sample in frame.iter_mut() {
            *sample -= mean;
        }

        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        if rms > f32::EPSILON {
            let desired_gain = (self.target_rms / rms).clamp(0.25, 4.0);
            self.last_gain =
                self.smoothing * self.last_gain + (1.0 - self.smoothing) * desired_gain;
        } else {
            self.last_gain = 1.0;
        }

        for sample in frame.iter_mut() {
            *sample = (*sample * self.last_gain).clamp(-1.0, 1.0);
        }
    }
}

#[derive(Debug)]
enum EnhancedStage {
    #[cfg(feature = "enhanced-denoise")]
    Dtln(DtlnBackend),
    Stub(EnhancedStub),
}

impl EnhancedStage {
    fn new() -> Self {
        #[cfg(feature = "enhanced-denoise")]
        {
            if let Some(backend) = DtlnBackend::new() {
                return EnhancedStage::Dtln(backend);
            }
        }
        EnhancedStage::Stub(EnhancedStub::new())
    }

    fn process(&mut self, frame: &mut [f32]) {
        match self {
            #[cfg(feature = "enhanced-denoise")]
            EnhancedStage::Dtln(backend) => backend.process(frame),
            EnhancedStage::Stub(stub) => stub.process(frame),
        }
    }
}

#[cfg(feature = "enhanced-denoise")]
#[derive(Debug)]
struct DtlnBackend {
    // Placeholder for future dtln-rs integration.
}

#[cfg(feature = "enhanced-denoise")]
impl DtlnBackend {
    fn new() -> Option<Self> {
        // TODO(logan): integrate dtln-rs once model packaging is finalized.
        warn!("dtln backend requested but not yet implemented; using fallback denoiser.");
        None
    }

    fn process(&mut self, frame: &mut [f32]) {
        let _ = frame;
    }
}

#[derive(Debug)]
struct EnhancedStub {
    alpha: f32,
    previous: f32,
}

impl EnhancedStub {
    fn new() -> Self {
        Self {
            alpha: 0.92,
            previous: 0.0,
        }
    }

    fn process(&mut self, frame: &mut [f32]) {
        for sample in frame.iter_mut() {
            let denoised = self.alpha * self.previous + (1.0 - self.alpha) * *sample;
            self.previous = denoised;
            *sample = denoised;
        }
    }
}
