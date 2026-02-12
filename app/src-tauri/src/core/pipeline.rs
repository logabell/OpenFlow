use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sysinfo::System;
use tauri::AppHandle;
use tracing::{info, warn};

use crate::asr::{AsrConfig, AsrEngine, RecognitionResult};
use crate::audio::{AudioEvent, AudioPipeline, AudioPipelineConfig, AudioPreprocessor};
use crate::core::events;
use crate::llm::{AutocleanMode, AutocleanService};
#[cfg(debug_assertions)]
use crate::output::logs;
use crate::output::{OutputAction, OutputInjector, PasteShortcut};
use crate::vad::{VadBackend, VadConfig, VadDecision, VadObservation, VoiceActivityDetector};

struct DiagnosticsState {
    last_emit: Instant,
    frames: u32,
    rms_sum: f32,
    peak_max: f32,
    vad: Option<VadObservation>,
}

#[derive(Debug)]
struct AudioWatchdogState {
    last_frame_ingress: Instant,
    seen_frame: bool,
    consecutive_restarts: u32,
    last_restart_attempt: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct NoOutputReason {
    code: &'static str,
    message: &'static str,
}

const VAD_MIN_SPEECH_MS: u64 = 350;
const VAD_PRE_ROLL_MS: u64 = 200;
const VAD_POST_ROLL_MS: u64 = 500;
const VAD_MAX_TRAILING_SILENCE_MS: u64 = 600;
const AUDIO_INGRESS_STALE_THRESHOLD: Duration = Duration::from_secs(2);
const AUDIO_WATCHDOG_TICK: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
struct VadTrimState {
    total_samples: usize,
    buffer_start: usize,
    first_active: Option<usize>,
    last_active: Option<usize>,
    active_samples: usize,
}

impl VadTrimState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn record(&mut self, decision: VadDecision, frame_samples: usize) {
        let start = self.total_samples;
        let end = start.saturating_add(frame_samples);

        if matches!(decision, VadDecision::Active) {
            if self.first_active.is_none() {
                self.first_active = Some(start);
            }
            self.last_active = Some(end);
            self.active_samples = self.active_samples.saturating_add(frame_samples);
        }

        self.total_samples = end;
    }

    fn note_buffer_drop(&mut self, dropped: usize) {
        if dropped == 0 {
            return;
        }
        self.buffer_start = self.buffer_start.saturating_add(dropped);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineMetrics {
    pub last_latency: Duration,
    pub consecutive_slow: u32,
    pub performance_mode: bool,
    pub average_cpu: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    Paste,
    EmitOnly,
}

impl Default for OutputMode {
    fn default() -> Self {
        OutputMode::Paste
    }
}

impl Default for EngineMetrics {
    fn default() -> Self {
        Self {
            last_latency: Duration::from_millis(0),
            consecutive_slow: 0,
            performance_mode: false,
            average_cpu: 0.0,
        }
    }
}

#[derive(Clone)]
pub struct SpeechPipeline {
    inner: Arc<SpeechPipelineInner>,
}

struct SpeechPipelineInner {
    audio: AudioPipeline,
    preprocessor: Mutex<AudioPreprocessor>,
    vad: Mutex<VoiceActivityDetector>,
    vad_default_hangover: Mutex<Duration>,
    vad_trim: Mutex<VadTrimState>,
    asr: AsrEngine,
    autoclean: AutocleanService,
    injector: OutputInjector,
    output_mode: Mutex<OutputMode>,
    metrics: Arc<Mutex<EngineMetrics>>,
    mode: Arc<Mutex<AutocleanMode>>,
    app: AppHandle,
    audio_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    listening: AtomicBool,
    diagnostics: Mutex<DiagnosticsState>,
    audio_watchdog: Mutex<AudioWatchdogState>,
}

impl SpeechPipeline {
    pub fn new(
        app: AppHandle,
        audio_config: AudioPipelineConfig,
        vad_config: VadConfig,
        asr_config: AsrConfig,
    ) -> Self {
        let preprocessor = AudioPreprocessor::new();
        let audio = AudioPipeline::spawn(audio_config);
        let vad = VoiceActivityDetector::new(vad_config.clone());
        let injector = OutputInjector::new();
        injector.prewarm();
        let inner = Arc::new(SpeechPipelineInner {
            audio,
            preprocessor: Mutex::new(preprocessor),
            vad: Mutex::new(vad),
            vad_default_hangover: Mutex::new(vad_config.hangover),
            vad_trim: Mutex::new(VadTrimState::default()),
            asr: AsrEngine::new(asr_config),
            autoclean: AutocleanService::new(),
            injector,
            output_mode: Mutex::new(OutputMode::default()),
            metrics: Arc::new(Mutex::new(EngineMetrics::default())),
            mode: Arc::new(Mutex::new(AutocleanMode::Fast)),
            app,
            audio_thread: Mutex::new(None),
            listening: AtomicBool::new(false),
            diagnostics: Mutex::new(DiagnosticsState {
                last_emit: Instant::now(),
                frames: 0,
                rms_sum: 0.0,
                peak_max: 0.0,
                vad: None,
            }),
            audio_watchdog: Mutex::new(AudioWatchdogState {
                last_frame_ingress: Instant::now(),
                seen_frame: false,
                consecutive_restarts: 0,
                last_restart_attempt: None,
            }),
        });

        SpeechPipelineInner::start_audio_loop(&inner);
        SpeechPipelineInner::start_cpu_sampler(&inner);
        SpeechPipelineInner::start_audio_watchdog(&inner);

        Self { inner }
    }

    pub fn audio_device_id(&self) -> Option<String> {
        self.inner.audio.device_id()
    }

    pub fn set_mode(&self, mode: AutocleanMode) {
        self.inner.set_mode(mode)
    }

    pub fn set_vad_config(&self, config: VadConfig) {
        self.inner.set_vad_config(config);
    }

    pub fn set_paste_shortcut(&self, shortcut: PasteShortcut) {
        self.inner.set_paste_shortcut(shortcut);
    }

    pub fn asr_config(&self) -> AsrConfig {
        self.inner.asr_config()
    }

    pub fn set_listening(&self, active: bool) {
        self.inner.set_listening(active);
    }

    pub fn has_recent_audio_ingress(&self, max_age: Duration) -> bool {
        self.inner.has_recent_audio_ingress(max_age)
    }

    pub fn set_output_mode(&self, mode: OutputMode) {
        self.inner.set_output_mode(mode);
    }

    pub fn warmup_asr(&self) -> Result<()> {
        self.inner.asr.warmup()?;
        Ok(())
    }
}

impl SpeechPipelineInner {
    fn start_audio_loop(this: &Arc<Self>) {
        let receiver = this.audio.subscribe();
        let weak = Arc::downgrade(this);
        let handle = std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                if let Some(inner) = weak.upgrade() {
                    if let Err(error) = inner.process_frame(event) {
                        warn!("audio frame processing failed: {error:?}");
                    }
                } else {
                    break;
                }
            }
        });

        let mut guard = this.audio_thread.lock();
        *guard = Some(handle);
    }

    fn set_output_mode(&self, mode: OutputMode) {
        let mut guard = self.output_mode.lock();
        *guard = mode;
    }

    fn start_cpu_sampler(this: &Arc<Self>) {
        let weak = Arc::downgrade(this);
        tauri::async_runtime::spawn(async move {
            let mut system = System::new();
            system.refresh_cpu_usage();
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            // The first measurement after refresh_cpu_usage is usually 0; wait a cycle.
            interval.tick().await;

            loop {
                interval.tick().await;
                if let Some(inner) = weak.upgrade() {
                    system.refresh_cpu_usage();
                    let usage = system.global_cpu_info().cpu_usage() / 100.0;
                    inner.record_cpu_load(usage.clamp(0.0, 1.0));
                } else {
                    break;
                }
            }
        });
    }

    fn start_audio_watchdog(this: &Arc<Self>) {
        let weak = Arc::downgrade(this);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(AUDIO_WATCHDOG_TICK);
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                inner.tick_audio_watchdog();
            }
        });
    }

    fn tick_audio_watchdog(&self) {
        if self.audio.is_synthetic() {
            return;
        }

        let now = Instant::now();
        let elapsed = {
            let guard = self.audio_watchdog.lock();
            now.duration_since(guard.last_frame_ingress)
        };

        if elapsed < AUDIO_INGRESS_STALE_THRESHOLD {
            return;
        }

        let cooldown = {
            let guard = self.audio_watchdog.lock();
            let shift = guard.consecutive_restarts.min(4);
            Duration::from_secs(2u64 << shift)
        };

        {
            let guard = self.audio_watchdog.lock();
            if let Some(last) = guard.last_restart_attempt {
                if now.duration_since(last) < cooldown {
                    return;
                }
            }
        }

        info!(
            "audio_watchdog_stale elapsed_ms={} cooldown_ms={}",
            elapsed.as_millis(),
            cooldown.as_millis()
        );

        let restart = self.audio.restart_capture();
        match restart {
            Ok(true) => {
                let mut guard = self.audio_watchdog.lock();
                guard.consecutive_restarts = guard.consecutive_restarts.saturating_add(1);
                guard.last_restart_attempt = Some(now);
                info!(
                    "audio_watchdog_restart_success attempts={} sample_rate={}",
                    guard.consecutive_restarts,
                    self.audio.sample_rate()
                );
            }
            Ok(false) => {
                let mut guard = self.audio_watchdog.lock();
                guard.last_restart_attempt = Some(now);
                info!("audio_watchdog_restart_skipped");
            }
            Err(error) => {
                let mut guard = self.audio_watchdog.lock();
                guard.consecutive_restarts = guard.consecutive_restarts.saturating_add(1);
                guard.last_restart_attempt = Some(now);
                warn!("audio_watchdog_restart_failed error={error}");
            }
        }
    }

    fn note_audio_ingress(&self) {
        let mut guard = self.audio_watchdog.lock();
        guard.last_frame_ingress = Instant::now();
        guard.seen_frame = true;
        guard.consecutive_restarts = 0;
    }

    fn has_recent_audio_ingress(&self, max_age: Duration) -> bool {
        if self.audio.is_synthetic() {
            return true;
        }
        let guard = self.audio_watchdog.lock();
        guard.seen_frame && guard.last_frame_ingress.elapsed() <= max_age
    }

    fn process_frame(&self, frame: AudioEvent) -> Result<()> {
        match frame {
            AudioEvent::Frame(mut samples) => {
                self.note_audio_ingress();
                if !self.listening.load(Ordering::Relaxed) {
                    return Ok(());
                }

                {
                    let mut preprocessor = self.preprocessor.lock();
                    preprocessor.process(&mut samples);
                }

                let vad_observation = {
                    let mut detector = self.vad.lock();
                    detector.evaluate(&samples)
                };

                self.record_diagnostics(&samples, vad_observation);

                {
                    let mut trim = self.vad_trim.lock();
                    trim.record(vad_observation.decision, samples.len());
                }

                // Always buffer audio while listening. VAD is used for diagnostics
                // and trimming, but shouldn't block push-to-talk dictation.
                let dropped = self.asr.push_samples(&samples);
                if dropped > 0 {
                    let mut trim = self.vad_trim.lock();
                    trim.note_buffer_drop(dropped);
                }
                Ok(())
            }
            AudioEvent::Stopped => {
                info!("audio stream stopped");
                Ok(())
            }
        }
    }

    fn record_diagnostics(&self, samples: &[f32], vad: VadObservation) {
        let (rms, peak) = compute_rms_peak(samples);
        let now = Instant::now();

        let (avg_rms, peak_max, vad_snapshot, should_emit) = {
            let mut diag = self.diagnostics.lock();
            diag.frames = diag.frames.saturating_add(1);
            diag.rms_sum += rms;
            diag.peak_max = diag.peak_max.max(peak);
            diag.vad = Some(vad);

            let should_emit = now.duration_since(diag.last_emit) >= Duration::from_millis(250);
            if !should_emit {
                return;
            }

            let frames = diag.frames.max(1) as f32;
            let avg_rms = (diag.rms_sum / frames).clamp(0.0, 1.0);
            let peak_max = diag.peak_max.clamp(0.0, 1.0);
            let vad_snapshot = diag.vad;

            diag.frames = 0;
            diag.rms_sum = 0.0;
            diag.peak_max = 0.0;
            diag.last_emit = now;

            (avg_rms, peak_max, vad_snapshot, true)
        };

        if should_emit {
            events::emit_audio_diagnostics(
                &self.app,
                events::AudioDiagnosticsPayload {
                    sample_rate: self.audio.sample_rate(),
                    device_id: self.audio.device_id(),
                    synthetic: self.audio.is_synthetic(),
                    rms: avg_rms,
                    peak: peak_max,
                },
            );

            if let Some(vad) = vad_snapshot {
                let backend = match vad.backend {
                    VadBackend::Silero => "silero",
                    VadBackend::Energy => "energy",
                };

                events::emit_vad_diagnostics(
                    &self.app,
                    events::VadDiagnosticsPayload {
                        backend: backend.to_string(),
                        active: matches!(vad.decision, VadDecision::Active),
                        score: vad.score,
                        threshold: vad.threshold,
                        hangover_ms: vad.hangover.as_millis() as u64,
                    },
                );
            }
        }
    }

    fn update_metrics(&self, latency: Duration) {
        let mut metrics = self.metrics.lock();
        metrics.last_latency = latency;

        if latency > Duration::from_secs(2) && metrics.average_cpu > 0.75 {
            metrics.consecutive_slow += 1;
            if metrics.consecutive_slow >= 2 && !metrics.performance_mode {
                metrics.performance_mode = true;
                self.set_performance_override(true);
                warn!("Entering performance warning mode");
                events::emit_performance_warning(&self.app, &*metrics);
                #[cfg(debug_assertions)]
                logs::push_log(format!(
                    "Performance warning: latency={}ms cpu={:.1}%",
                    latency.as_millis(),
                    metrics.average_cpu * 100.0
                ));
            }
        } else {
            metrics.consecutive_slow = 0;
            if metrics.performance_mode {
                info!("recovering from performance warning");
                metrics.performance_mode = false;
                self.set_performance_override(false);
                events::emit_performance_recovered(&self.app, &*metrics);
                #[cfg(debug_assertions)]
                logs::push_log("Performance recovered".to_string());
            }
        }

        events::emit_metrics(&self.app, &*metrics);
    }

    fn record_cpu_load(&self, cpu_fraction: f32) {
        let mut metrics = self.metrics.lock();
        metrics.average_cpu = cpu_fraction;
        if metrics.average_cpu < 0.75 && metrics.performance_mode {
            metrics.performance_mode = false;
            metrics.consecutive_slow = 0;
            info!("Performance warning cleared by CPU recovery");
            self.set_performance_override(false);
            events::emit_performance_recovered(&self.app, &*metrics);
        }

        events::emit_metrics(&self.app, &*metrics);
    }

    fn set_mode(&self, mode: AutocleanMode) {
        let mut guard = self.mode.lock();
        *guard = mode;
        self.autoclean.set_mode(mode);
    }

    fn set_vad_config(&self, config: VadConfig) {
        let mut vad = self.vad.lock();
        *vad = VoiceActivityDetector::new(config.clone());
        let mut default = self.vad_default_hangover.lock();
        *default = config.hangover;
    }

    fn set_performance_override(&self, enabled: bool) {
        {
            let mut vad = self.vad.lock();
            let default = *self.vad_default_hangover.lock();
            if enabled {
                vad.set_hangover(default.min(Duration::from_millis(200)));
            } else {
                vad.set_hangover(default);
            }
        }
    }

    fn reset_recognizer(&self) {
        self.asr.reset();
    }

    fn reset_vad(&self) {
        let mut vad = self.vad.lock();
        vad.reset();
    }

    fn reset_trim_state(&self) {
        let mut trim = self.vad_trim.lock();
        trim.reset();
    }

    fn set_paste_shortcut(&self, shortcut: PasteShortcut) {
        self.injector.set_paste_shortcut(shortcut);
    }

    fn asr_config(&self) -> AsrConfig {
        self.asr.config().clone()
    }

    fn emit_no_output_reason(&self, reason: NoOutputReason) {
        info!(
            "dictation_no_output reason={} message={}",
            reason.code, reason.message
        );
        events::emit_transcription_skipped(&self.app, reason.code, reason.message);
        #[cfg(debug_assertions)]
        logs::push_log(format!("No output: {} ({})", reason.message, reason.code));
    }

    fn compute_trim_range(
        &self,
        sample_rate: u32,
        buffer_len: usize,
    ) -> Result<(usize, usize), NoOutputReason> {
        if buffer_len == 0 {
            return Err(NoOutputReason {
                code: "no-audio",
                message: "No audio captured; skipping ASR",
            });
        }

        let trim = self.vad_trim.lock();
        let min_samples = ((VAD_MIN_SPEECH_MS * sample_rate as u64) / 1000) as usize;
        if trim.first_active.is_none() || trim.active_samples < min_samples {
            return Err(NoOutputReason {
                code: "no-speech",
                message: "No speech detected; skipping ASR",
            });
        }

        let first = trim.first_active.unwrap_or(0);
        let last = trim.last_active.unwrap_or(first);
        let pre_roll = ((VAD_PRE_ROLL_MS * sample_rate as u64) / 1000) as usize;
        let post_roll = ((VAD_POST_ROLL_MS * sample_rate as u64) / 1000) as usize;
        let keep_tail = ((VAD_MAX_TRAILING_SILENCE_MS * sample_rate as u64) / 1000) as usize;

        let start_abs = first.saturating_sub(pre_roll);
        let mut end_abs = last.saturating_add(post_roll);

        let buffer_start = trim.buffer_start;
        let buffer_end = buffer_start.saturating_add(buffer_len);

        let trailing_silence = buffer_end.saturating_sub(last);
        if trailing_silence <= keep_tail {
            end_abs = buffer_end;
        }
        let start = start_abs.max(buffer_start);
        let end = end_abs.min(buffer_end);

        if end <= start {
            return Err(NoOutputReason {
                code: "trim-rejected",
                message: "Speech trim rejected; skipping ASR",
            });
        }

        Ok((start - buffer_start, end - buffer_start))
    }

    fn set_listening(&self, active: bool) {
        if active {
            self.listening.store(true, Ordering::SeqCst);
            self.reset_recognizer();
            self.reset_vad();
            self.reset_trim_state();
            return;
        }

        let was_listening = self.listening.swap(false, Ordering::SeqCst);
        if !was_listening {
            self.reset_recognizer();
            self.reset_vad();
            self.reset_trim_state();
            return;
        }

        let sample_rate = self.audio.sample_rate();
        let samples = self.asr.take_samples();
        #[cfg(debug_assertions)]
        {
            let pending = samples.len();
            logs::push_log(format!(
                "Finalizing dictation (samples={} rate={}Hz)",
                pending, sample_rate
            ));
        }

        let trim_range = self.compute_trim_range(sample_rate, samples.len());
        let (trim_start, trim_end) = match trim_range {
            Ok(range) => range,
            Err(reason) => {
                self.emit_no_output_reason(reason);
                self.reset_recognizer();
                self.reset_vad();
                self.reset_trim_state();
                return;
            }
        };

        let trimmed_samples = &samples[trim_start..trim_end];

        match self.asr.finalize_samples(sample_rate, trimmed_samples) {
            Ok(Some(result)) => {
                if result.text.trim().is_empty() {
                    self.emit_no_output_reason(NoOutputReason {
                        code: "empty-transcript",
                        message: "ASR returned empty transcript",
                    });
                    events::emit_transcription_error(&self.app, "ASR returned empty transcript");
                    #[cfg(debug_assertions)]
                    logs::push_log("ASR returned empty transcript".to_string());
                }
                self.consume_result(result);
            }
            Ok(None) => {
                self.emit_no_output_reason(NoOutputReason {
                    code: "no-speech",
                    message: "No speech detected; skipping ASR",
                });
            }
            Err(error) => {
                events::emit_transcription_error(&self.app, &error.to_string());
                #[cfg(debug_assertions)]
                logs::push_log(format!("ASR error: {error}"));
            }
        }
        self.reset_recognizer();
        self.reset_vad();
        self.reset_trim_state();
    }

    fn consume_result(&self, recognition: RecognitionResult) {
        self.update_metrics(recognition.latency);

        let trimmed = recognition.text.trim();
        if trimmed.is_empty() {
            self.emit_no_output_reason(NoOutputReason {
                code: "empty-transcript",
                message: "ASR produced empty transcript",
            });
            return;
        }

        let active_mode = *self.mode.lock();
        self.autoclean.set_mode(active_mode);
        let cleaned = self.autoclean.clean(trimmed);
        self.deliver_output(&cleaned);
    }

    fn deliver_output(&self, cleaned: &str) {
        if cleaned.trim().is_empty() {
            self.emit_no_output_reason(NoOutputReason {
                code: "clean-empty",
                message: "Cleanup removed all transcript text",
            });
            return;
        }

        events::emit_transcription_output(&self.app, cleaned);
        #[cfg(debug_assertions)]
        logs::push_log(format!("Transcription -> {}", cleaned));

        let mode = *self.output_mode.lock();
        if matches!(mode, OutputMode::Paste) {
            let configured_shortcut = self.injector.current_paste_shortcut();
            let shortcut = match configured_shortcut {
                PasteShortcut::CtrlV => "ctrl-v",
                PasteShortcut::CtrlShiftV => "ctrl-shift-v",
            };

            match self.injector.inject(cleaned, OutputAction::Paste) {
                Ok(()) => {
                    events::emit_paste_succeeded(
                        &self.app,
                        events::PasteSucceededPayload {
                            shortcut: shortcut.to_string(),
                            chars: cleaned.len(),
                        },
                    );
                }
                Err(error) => {
                    let linux = Some(crate::core::linux_setup::permissions_status());

                    match error {
                        crate::output::OutputInjectionError::Paste(paste) => {
                            let payload = events::PasteFailedPayload {
                                step: paste.step.as_str().to_string(),
                                message: paste.message,
                                shortcut: shortcut.to_string(),
                                transcript_on_clipboard: paste.transcript_on_clipboard,
                                linux,
                            };

                            if matches!(paste.kind, crate::output::PasteFailureKind::Unconfirmed) {
                                events::emit_paste_unconfirmed(&self.app, payload);
                            } else {
                                events::emit_paste_failed(&self.app, payload);
                            }
                        }
                        crate::output::OutputInjectionError::Copy(message) => {
                            events::emit_paste_failed(
                                &self.app,
                                events::PasteFailedPayload {
                                    step: "clipboard".to_string(),
                                    message,
                                    shortcut: "unknown".to_string(),
                                    transcript_on_clipboard: false,
                                    linux,
                                },
                            );
                        }
                    }
                }
            }
        } else {
            #[cfg(debug_assertions)]
            logs::push_log("Output mode set to emit-only; skipping paste".to_string());
        }
    }
}

fn compute_rms_peak(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f32;
    for sample in samples {
        let abs = sample.abs();
        if abs > peak {
            peak = abs;
        }
        sum_sq += sample * sample;
    }
    let rms = (sum_sq / samples.len() as f32).sqrt();
    (rms, peak)
}

impl Drop for SpeechPipelineInner {
    fn drop(&mut self) {
        let handle = self.audio_thread.lock().take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}
