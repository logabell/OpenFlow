use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::core::linux_setup::LinuxPermissionsStatus;
use crate::core::pipeline::EngineMetrics;
use crate::llm::AutocleanMode;

pub const EVENT_HUD_STATE: &str = "hud-state";
pub const EVENT_PERFORMANCE_WARNING: &str = "performance-warning";
pub const EVENT_PERFORMANCE_RECOVERED: &str = "performance-recovered";
pub const EVENT_SECURE_BLOCKED: &str = "secure-field-blocked";

pub const EVENT_TRANSCRIPTION_OUTPUT: &str = "transcription-output";
pub const EVENT_TRANSCRIPTION_ERROR: &str = "transcription-error";
pub const EVENT_PERFORMANCE_METRICS: &str = "performance-metrics";
pub const EVENT_MODEL_STATUS: &str = "model-status";

pub const EVENT_PASTE_FAILED: &str = "paste-failed";
pub const EVENT_PASTE_UNCONFIRMED: &str = "paste-unconfirmed";
pub const EVENT_PASTE_SUCCEEDED: &str = "paste-succeeded";

pub const EVENT_AUDIO_DIAGNOSTICS: &str = "audio-diagnostics";
pub const EVENT_VAD_DIAGNOSTICS: &str = "vad-diagnostics";

pub const EVENT_UPDATE_DOWNLOAD_PROGRESS: &str = "update-download-progress";
pub const EVENT_UPDATE_APPLY_PROGRESS: &str = "update-apply-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteFailedPayload {
    pub step: String,
    pub message: String,
    pub shortcut: String,
    pub transcript_on_clipboard: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<LinuxPermissionsStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteSucceededPayload {
    pub shortcut: String,
    pub chars: usize,
}

pub fn emit_hud_state(app: &AppHandle, state: &str) {
    let _ = app.emit(EVENT_HUD_STATE, state.to_string());
}

pub fn emit_performance_warning(app: &AppHandle, metrics: &EngineMetrics) {
    let _ = app.emit(EVENT_PERFORMANCE_WARNING, metrics.clone());
}

pub fn emit_performance_recovered(app: &AppHandle, metrics: &EngineMetrics) {
    let _ = app.emit(EVENT_PERFORMANCE_RECOVERED, metrics.clone());
}

pub fn emit_secure_blocked(app: &AppHandle) {
    let _ = app.emit(EVENT_SECURE_BLOCKED, ());
}

pub fn emit_autoclean_mode(app: &AppHandle, mode: AutocleanMode) {
    let _ = app.emit("autoclean-mode", mode);
}

pub fn emit_transcription_output(app: &AppHandle, text: &str) {
    let _ = app.emit(EVENT_TRANSCRIPTION_OUTPUT, text.to_string());
}

pub fn emit_transcription_error(app: &AppHandle, message: &str) {
    let _ = app.emit(EVENT_TRANSCRIPTION_ERROR, message.to_string());
}

pub fn emit_paste_failed(app: &AppHandle, payload: PasteFailedPayload) {
    let _ = app.emit(EVENT_PASTE_FAILED, payload);
}

pub fn emit_paste_unconfirmed(app: &AppHandle, payload: PasteFailedPayload) {
    let _ = app.emit(EVENT_PASTE_UNCONFIRMED, payload);
}

pub fn emit_paste_succeeded(app: &AppHandle, payload: PasteSucceededPayload) {
    let _ = app.emit(EVENT_PASTE_SUCCEEDED, payload);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsPayload {
    pub sample_rate: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    pub synthetic: bool,
    pub rms: f32,
    pub peak: f32,
}

pub fn emit_audio_diagnostics(app: &AppHandle, payload: AudioDiagnosticsPayload) {
    let _ = app.emit(EVENT_AUDIO_DIAGNOSTICS, payload);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VadDiagnosticsPayload {
    pub backend: String,
    pub active: bool,
    pub score: f32,
    pub threshold: f32,
    pub hangover_ms: u64,
}

pub fn emit_vad_diagnostics(app: &AppHandle, payload: VadDiagnosticsPayload) {
    let _ = app.emit(EVENT_VAD_DIAGNOSTICS, payload);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricsPayload {
    last_latency_ms: u64,
    average_cpu_percent: f32,
    consecutive_slow: u32,
    performance_mode: bool,
}

pub fn emit_metrics(app: &AppHandle, metrics: &EngineMetrics) {
    let payload = MetricsPayload {
        last_latency_ms: metrics.last_latency.as_millis() as u64,
        average_cpu_percent: metrics.average_cpu * 100.0,
        consecutive_slow: metrics.consecutive_slow,
        performance_mode: metrics.performance_mode,
    };
    let _ = app.emit(EVENT_PERFORMANCE_METRICS, payload);
}

pub fn emit_model_status<T: Serialize + Clone>(app: &AppHandle, payload: T) {
    let _ = app.emit(EVENT_MODEL_STATUS, payload);
}

pub fn emit_update_download_progress(
    app: &AppHandle,
    payload: crate::core::updater::UpdateDownloadProgress,
) {
    let _ = app.emit(EVENT_UPDATE_DOWNLOAD_PROGRESS, payload);
}

pub fn emit_update_apply_progress(
    app: &AppHandle,
    payload: crate::core::updater::UpdateApplyProgress,
) {
    let _ = app.emit(EVENT_UPDATE_APPLY_PROGRESS, payload);
}
