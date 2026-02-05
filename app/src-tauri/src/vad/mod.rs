mod engine;
#[cfg(feature = "vad-silero")]
pub mod silero;

pub use engine::{VadBackend, VadConfig, VadDecision, VadObservation, VoiceActivityDetector};
