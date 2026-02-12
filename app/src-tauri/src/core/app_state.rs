use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;

use crate::asr::{AsrBackend, AsrConfig};
use crate::audio::AudioPipelineConfig;
use crate::core::events;
use crate::llm::AutocleanMode;
use crate::models::{
    sync_runtime_environment, ModelDownloadJob, ModelDownloadService, ModelKind, ModelManager,
    ModelStatus,
};
use crate::output::PasteShortcut;
use crate::vad::VadConfig;
use tauri::WebviewUrl;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindowBuilder};
use tracing::{debug, warn};

use super::pipeline::{OutputMode, SpeechPipeline};
use super::settings::{AsrSelection, SettingsManager};

fn env_flag_enabled(key: &str) -> bool {
    let value = match std::env::var(key) {
        Ok(value) => value,
        Err(_) => return false,
    };

    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

fn disable_asr_warmup() -> bool {
    env_flag_enabled("OPENFLOW_TEST_MODE") || env_flag_enabled("OPENFLOW_DISABLE_ASR_WARMUP")
}

fn disable_model_autodownload() -> bool {
    env_flag_enabled("OPENFLOW_TEST_MODE")
        || env_flag_enabled("OPENFLOW_DISABLE_MODEL_AUTODOWNLOAD")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrWarmupState {
    Warming,
    Ready,
    Error,
}

#[derive(Debug, Clone)]
struct AsrWarmupTracker {
    state: AsrWarmupState,
    warmed_selection: Option<AsrSelection>,
    target_selection: Option<AsrSelection>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Listening,
    Processing,
}

pub struct AppState {
    settings: Arc<SettingsManager>,
    pipeline: Arc<Mutex<Option<SpeechPipeline>>>,
    session: Arc<Mutex<SessionState>>,
    models: Arc<StdMutex<ModelManager>>,
    downloads: Arc<Mutex<Option<ModelDownloadService>>>,
    hud_state: Arc<Mutex<String>>,
    asr_warmup: Arc<Mutex<AsrWarmupTracker>>,
    asr_warmup_generation: Arc<AtomicU64>,
}

impl AppState {
    pub fn new() -> Self {
        let models = ModelManager::new().expect("failed to initialize model manager");
        let warmup_state = if disable_asr_warmup() {
            AsrWarmupState::Ready
        } else {
            AsrWarmupState::Warming
        };
        Self {
            settings: Arc::new(SettingsManager::new()),
            pipeline: Arc::new(Mutex::new(None)),
            session: Arc::new(Mutex::new(SessionState::Idle)),
            models: Arc::new(StdMutex::new(models)),
            downloads: Arc::new(Mutex::new(None)),
            hud_state: Arc::new(Mutex::new("idle".to_string())),
            asr_warmup: Arc::new(Mutex::new(AsrWarmupTracker {
                state: warmup_state,
                warmed_selection: None,
                target_selection: None,
                last_error: None,
            })),
            asr_warmup_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn settings_manager(&self) -> Arc<SettingsManager> {
        self.settings.clone()
    }

    pub fn model_manager(&self) -> Arc<StdMutex<ModelManager>> {
        self.models.clone()
    }

    pub fn set_hud_state(&self, app: &AppHandle, state: &str) {
        {
            let mut guard = self.hud_state.lock();
            *guard = state.to_string();
        }
        publish_hud_runtime_state(self, state);
        events::emit_hud_state(app, state);
    }

    pub fn sync_hud_overlay_mode(&self, app: &AppHandle) {
        let hud_state = { self.hud_state.lock().clone() };
        publish_hud_runtime_state(self, &hud_state);

        if !window_overlay_supported() {
            hide_status_overlay(app);
            return;
        }

        let show_overlay = self
            .settings_manager()
            .read_frontend()
            .map(|settings| settings.show_hud_overlay)
            .unwrap_or(false);

        if !show_overlay || hud_state == "idle" {
            hide_status_overlay(app);
            return;
        }

        show_status_overlay(app, overlay_monitor_target_from_cursor(app));
    }

    pub fn replay_hud_state(&self, app: &AppHandle) {
        let state = { self.hud_state.lock().clone() };
        events::emit_hud_state(app, &state);
    }

    pub fn asr_warmup_state(&self) -> AsrWarmupState {
        self.asr_warmup.lock().state
    }

    pub fn kickoff_asr_warmup(&self, app: &AppHandle) {
        if disable_asr_warmup() {
            let selection = self
                .settings
                .read_frontend()
                .ok()
                .map(|s| AsrSelection::from_frontend(&s));
            let mut tracker = self.asr_warmup.lock();
            tracker.state = AsrWarmupState::Ready;
            tracker.warmed_selection = selection.clone();
            tracker.target_selection = selection;
            tracker.last_error = None;
            return;
        }

        let settings = match self.settings.read_frontend() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!("Failed to read settings for ASR warmup: {error:?}");
                let mut tracker = self.asr_warmup.lock();
                tracker.state = AsrWarmupState::Ready;
                tracker.last_error = Some(error.to_string());
                return;
            }
        };

        let selection = AsrSelection::from_frontend(&settings);
        let should_start = {
            let mut tracker = self.asr_warmup.lock();
            if tracker.state == AsrWarmupState::Ready {
                if tracker.warmed_selection.as_ref() == Some(&selection) {
                    return;
                }
            }
            if tracker.state == AsrWarmupState::Warming {
                if tracker.target_selection.as_ref() == Some(&selection) {
                    return;
                }
            }

            tracker.state = AsrWarmupState::Warming;
            tracker.target_selection = Some(selection);
            tracker.last_error = None;
            true
        };

        if !should_start {
            return;
        }

        let generation = self.asr_warmup_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let app_handle = app.clone();

        tauri::async_runtime::spawn(async move {
            let started = Instant::now();

            // Read current selection for logging.
            let selection_label = {
                let state = app_handle.state::<AppState>();
                state
                    .settings_manager()
                    .read_frontend()
                    .map(|s| format_asr_selection_label(&s))
                    .unwrap_or_else(|_| "unknown".to_string())
            };

            tracing::info!("asr_warmup_start model={selection_label}");

            let result = warmup_current_asr(&app_handle, generation).await;
            match result {
                Ok(()) => {
                    tracing::info!(
                        "asr_warmup_end model={selection_label} duration_ms={}",
                        started.elapsed().as_millis() as u64
                    );
                }
                Err(error) => {
                    tracing::info!("asr_warmup_failed model={selection_label} error={}", error);
                }
            }
        });
    }

    pub fn start_session(&self, app: &AppHandle) {
        let show_overlay = self
            .settings_manager()
            .read_frontend()
            .map(|settings| settings.show_hud_overlay)
            .unwrap_or(false);

        self.start_session_with_overlay(app, show_overlay);
    }

    pub fn start_session_with_overlay(&self, app: &AppHandle, show_overlay: bool) {
        let use_window_overlay = show_overlay && window_overlay_supported();
        let target_monitor = if use_window_overlay {
            overlay_monitor_target_from_cursor(app)
        } else {
            None
        };

        match self.asr_warmup_state() {
            AsrWarmupState::Warming => {
                tracing::info!("hotkey_ignored_engine_warming");
                if use_window_overlay {
                    show_status_overlay(app, target_monitor);
                } else {
                    hide_status_overlay(app);
                }
                self.set_hud_state(app, "warming");
                return;
            }
            AsrWarmupState::Error => {
                tracing::warn!("hotkey_ignored_engine_error");
                if use_window_overlay {
                    show_status_overlay(app, target_monitor);
                } else {
                    hide_status_overlay(app);
                }
                self.set_hud_state(app, "asr-error");
                return;
            }
            AsrWarmupState::Ready => {}
        }

        let should_start = {
            let mut guard = self.session.lock();
            // Only start a new session from Idle. If we're already listening or
            // processing, ignore the request.
            if *guard != SessionState::Idle {
                false
            } else {
                *guard = SessionState::Listening;
                true
            }
        };
        if !should_start {
            return;
        }

        // Don't hold the pipeline mutex while toggling listening.
        let pipeline = { self.pipeline.lock().as_ref().cloned() };
        if let Some(pipeline) = pipeline {
            pipeline.set_listening(true);
        }

        if use_window_overlay {
            show_status_overlay(app, target_monitor);
        } else if app.get_webview_window("status-overlay").is_some() {
            // Make sure a previously-shown overlay can't steal focus/cancel input
            // while using debug hold-to-talk.
            hide_status_overlay(app);
        }

        self.set_hud_state(app, "listening");
    }

    pub fn mark_processing(&self, app: &AppHandle) {
        let mut guard = self.session.lock();
        if *guard != SessionState::Listening {
            return;
        }
        *guard = SessionState::Processing;
        self.set_hud_state(app, "processing");
    }

    pub fn complete_session(&self, app: &AppHandle) {
        let previous = {
            let mut guard = self.session.lock();
            let prev = *guard;

            match prev {
                SessionState::Idle => {
                    // Ensure we still hide overlay + stop any lingering audio capture.
                }
                SessionState::Listening => {
                    // If callers didn't explicitly mark processing, do it here so the
                    // HUD reflects we're finalizing.
                    *guard = SessionState::Processing;
                }
                SessionState::Processing => {
                    // Keep processing state until finalize completes.
                }
            }

            prev
        };

        if matches!(previous, SessionState::Listening) {
            self.set_hud_state(app, "processing");
        }

        // Clone the pipeline handle so we can finalize without holding the mutex.
        let pipeline = { self.pipeline.lock().as_ref().cloned() };
        let app_handle = app.clone();
        let session = self.session.clone();

        // If we weren't in an active session, still force-hide the overlay immediately.
        if matches!(previous, SessionState::Idle) {
            hide_status_overlay(app);
            self.set_hud_state(app, "idle");
        }

        tauri::async_runtime::spawn(async move {
            if let Some(pipeline) = pipeline {
                if let Err(error) = tokio::task::spawn_blocking(move || {
                    pipeline.set_listening(false);
                })
                .await
                {
                    warn!("failed to finalize dictation: {error:?}");
                }
            } else {
                debug!("complete_session: pipeline not initialized");
            }

            {
                let mut guard = session.lock();
                *guard = SessionState::Idle;
            }

            if let Some(state) = app_handle.try_state::<AppState>() {
                state.set_hud_state(&app_handle, "idle");

                // Let the frontend play a short exit animation before hiding the
                // overlay window. Guard against races with a new dictation start.
                tokio::time::sleep(std::time::Duration::from_millis(260)).await;
                let still_idle = {
                    let hud = state.hud_state.lock();
                    hud.as_str() == "idle"
                };
                if still_idle {
                    hide_status_overlay(&app_handle);
                }
            } else {
                events::emit_hud_state(&app_handle, "idle");
                tokio::time::sleep(std::time::Duration::from_millis(260)).await;
                hide_status_overlay(&app_handle);
            }
        });
    }

    pub fn secure_blocked(&self, app: &AppHandle) {
        events::emit_secure_blocked(app);
        self.complete_session(app);
    }

    pub fn set_output_mode(&self, mode: OutputMode) -> Result<()> {
        let guard = self.pipeline.lock();
        let pipeline = guard
            .as_ref()
            .ok_or_else(|| anyhow!("pipeline not initialized"))?;
        pipeline.set_output_mode(mode);
        Ok(())
    }

    pub fn is_listening(&self) -> bool {
        matches!(*self.session.lock(), SessionState::Listening)
    }

    pub fn hotkey_mode(&self) -> String {
        self.settings
            .read_frontend()
            .map(|settings| settings.hotkey_mode)
            .unwrap_or_else(|_| "hold".into())
    }

    pub fn initialize_pipeline(&self, app: &AppHandle) -> Result<()> {
        self.sync_model_environment();
        let settings = self.settings.read_frontend()?;
        self.configure_pipeline(Some(app), &settings)
    }

    pub fn configure_pipeline(
        &self,
        app: Option<&AppHandle>,
        settings: &crate::core::settings::FrontendSettings,
    ) -> Result<()> {
        let desired_asr_config = self.build_asr_config(settings);
        let desired_paste_shortcut = parse_paste_shortcut(&settings.paste_shortcut);
        let mut guard = self.pipeline.lock();
        if let Some(existing) = guard.as_ref() {
            let desired_device = settings.audio_device_id.clone();
            if existing.audio_device_id() != desired_device
                || existing.asr_config() != desired_asr_config
            {
                *guard = None;
            }
        }

        let vad_config = VadConfig {
            sensitivity: settings.vad_sensitivity.clone(),
            ..VadConfig::default()
        };

        if let Some(pipeline) = guard.as_mut() {
            pipeline.set_mode(parse_autoclean_mode(&settings.autoclean_mode));
            pipeline.set_vad_config(vad_config.clone());
            pipeline.set_paste_shortcut(desired_paste_shortcut);
            if let Some(app) = app {
                events::emit_autoclean_mode(app, parse_autoclean_mode(&settings.autoclean_mode));
            }
            return Ok(());
        }

        let app = app.ok_or_else(|| anyhow!("app handle required to construct pipeline"))?;
        self.sync_model_environment();
        let audio_config = AudioPipelineConfig {
            device_id: settings.audio_device_id.clone(),
        };
        let pipeline = SpeechPipeline::new(
            app.clone(),
            audio_config,
            vad_config.clone(),
            desired_asr_config,
        );
        pipeline.set_mode(parse_autoclean_mode(&settings.autoclean_mode));
        pipeline.set_vad_config(vad_config);
        pipeline.set_paste_shortcut(desired_paste_shortcut);
        *guard = Some(pipeline);
        events::emit_autoclean_mode(app, parse_autoclean_mode(&settings.autoclean_mode));
        Ok(())
    }

    pub fn initialize_models(&self, app: &AppHandle) -> Result<()> {
        self.ensure_download_service(app)?;
        self.sync_model_environment();

        self.repair_installed_ct2_models(app);

        if !disable_model_autodownload() {
            // Auto-download default models if they're not installed
            self.auto_download_default_models(app);
        }

        Ok(())
    }

    fn repair_installed_ct2_models(&self, app: &AppHandle) {
        let mut snapshots = Vec::new();
        let result = {
            let mut guard = match self.models.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };

            let root = guard.root().to_path_buf();
            for asset in guard.assets_mut() {
                if asset.kind != ModelKind::WhisperCt2 {
                    continue;
                }
                if !matches!(asset.status, ModelStatus::Installed) {
                    continue;
                }

                let dir = asset.path(&root);
                if let Err(error) = crate::models::prepare_ct2_model_dir(&dir) {
                    asset.status =
                        ModelStatus::Error(format!("CT2 model invalid on disk: {error}"));
                    snapshots.push(asset.clone());
                }
            }

            guard.save()
        };

        if let Err(error) = result {
            tracing::warn!("Failed to repair CT2 models: {error:?}");
        }

        for snapshot in snapshots {
            events::emit_model_status(app, snapshot);
        }
    }

    fn auto_download_default_models(&self, app: &AppHandle) {
        let (parakeet_asset, parakeet_missing, vad_asset, vad_missing) = {
            let guard = match self.models.lock() {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("Failed to lock model manager: {e}");
                    return;
                }
            };

            let parakeet_asset = guard
                .primary_asset(&ModelKind::Parakeet)
                .map(|a| a.name.clone());
            let parakeet_missing = parakeet_asset
                .as_ref()
                .and_then(|name| guard.asset_by_name(name))
                .map(|a| !matches!(a.status, ModelStatus::Installed))
                .unwrap_or(true);

            let vad_asset = guard.primary_asset(&ModelKind::Vad).map(|a| a.name.clone());
            let vad_missing = vad_asset
                .as_ref()
                .and_then(|name| guard.asset_by_name(name))
                .map(|a| !matches!(a.status, ModelStatus::Installed))
                .unwrap_or(true);

            (parakeet_asset, parakeet_missing, vad_asset, vad_missing)
        };

        if parakeet_missing {
            tracing::info!("Parakeet ASR not installed, auto-downloading...");
            if let Some(name) = parakeet_asset {
                if let Err(e) = self.queue_model_download(app, &name) {
                    tracing::warn!("Failed to queue Parakeet download: {e:?}");
                }
            }
        }

        if vad_missing {
            tracing::info!("Silero VAD not installed, auto-downloading...");
            if let Some(name) = vad_asset {
                if let Err(e) = self.queue_model_download(app, &name) {
                    tracing::warn!("Failed to queue VAD download: {e:?}");
                }
            }
        }
    }

    pub fn queue_model_download(&self, app: &AppHandle, asset_name: &str) -> Result<()> {
        self.ensure_download_service(app)?;
        let service = self
            .downloads
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("download service unavailable"))?;
        service.queue(ModelDownloadJob {
            asset_name: asset_name.to_string(),
        })
    }

    pub fn reload_pipeline(&self, app: &AppHandle) -> Result<()> {
        let settings = self.settings.read_frontend()?;
        {
            let mut guard = self.pipeline.lock();
            *guard = None;
        }
        self.configure_pipeline(Some(app), &settings)
    }

    fn ensure_download_service(&self, app: &AppHandle) -> Result<()> {
        let mut guard = self.downloads.lock();
        if guard.is_none() {
            let manager = self.models.clone();
            let service = ModelDownloadService::new(app.clone(), manager)?;
            *guard = Some(service);
        }
        Ok(())
    }

    fn sync_model_environment(&self) {
        if let Ok(manager) = self.models.lock() {
            if let Err(error) = sync_runtime_environment(&*manager) {
                tracing::warn!("Failed to sync model runtime environment: {error:?}");
            }
        }
    }

    fn build_asr_config(&self, settings: &crate::core::settings::FrontendSettings) -> AsrConfig {
        let backend = parse_asr_backend(settings);
        let model_dir = self.resolve_asr_model_dir(settings, &backend);

        let provider = std::env::var("SHERPA_PROVIDER").unwrap_or_else(|_| "cpu".into());
        let num_threads = std::env::var("SHERPA_THREADS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .filter(|value| *value > 0);

        let ct2_device = std::env::var("CT2_DEVICE").unwrap_or_else(|_| "cpu".into());
        let ct2_compute_type = match settings.whisper_precision.as_str() {
            "float" => "float16".to_string(),
            _ => "int8".to_string(),
        };

        let (language, auto_language_detect) =
            if settings.asr_family == "whisper" && settings.whisper_model_language == "en" {
                ("en".to_string(), false)
            } else {
                (settings.language.clone(), settings.auto_detect_language)
            };

        AsrConfig {
            backend,
            language,
            auto_language_detect,
            model_dir,
            provider,
            num_threads,
            ct2_device,
            ct2_compute_type,
        }
    }

    fn resolve_asr_model_dir(
        &self,
        settings: &crate::core::settings::FrontendSettings,
        backend: &AsrBackend,
    ) -> Option<std::path::PathBuf> {
        let (kind, asset_name) = match *backend {
            AsrBackend::WhisperOnnx => (
                ModelKind::WhisperOnnx,
                resolve_whisper_asset_name(settings, backend),
            ),
            AsrBackend::WhisperCt2 => (
                ModelKind::WhisperCt2,
                resolve_whisper_asset_name(settings, backend),
            ),
            AsrBackend::Parakeet => (ModelKind::Parakeet, None),
        };

        self.models.lock().ok().and_then(|guard| {
            let asset = if let Some(name) = asset_name {
                guard.asset_by_name(&name)
            } else {
                guard.primary_asset(&kind)
            };

            asset.and_then(|asset| {
                if matches!(asset.status, ModelStatus::Installed) {
                    Some(asset.path(guard.root()))
                } else {
                    None
                }
            })
        })
    }

    pub fn uninstall_model(&self, app: &AppHandle, asset_name: &str) -> Result<()> {
        let snapshot = {
            let mut guard = self.models.lock().map_err(|err| anyhow!(err.to_string()))?;
            let result = guard.uninstall_by_name(asset_name)?;
            result
        };
        self.sync_model_environment();
        if let Some(asset) = snapshot {
            events::emit_model_status(app, asset);
        }
        self.reload_pipeline(app)?;
        Ok(())
    }
}

fn parse_autoclean_mode(value: &str) -> AutocleanMode {
    match value {
        "off" => AutocleanMode::Off,
        _ => AutocleanMode::Fast,
    }
}

fn parse_asr_backend(settings: &crate::core::settings::FrontendSettings) -> AsrBackend {
    if settings.asr_family == "whisper" {
        if settings.whisper_backend == "onnx" {
            AsrBackend::WhisperOnnx
        } else {
            AsrBackend::WhisperCt2
        }
    } else {
        AsrBackend::Parakeet
    }
}

fn resolve_whisper_asset_name(
    settings: &crate::core::settings::FrontendSettings,
    backend: &AsrBackend,
) -> Option<String> {
    let size = match settings.whisper_model.as_str() {
        "tiny" | "base" | "small" | "medium" | "large-v3" | "large-v3-turbo" => {
            settings.whisper_model.as_str()
        }
        _ => "small",
    };

    let language = if matches!(size, "large-v3" | "large-v3-turbo") {
        "multi"
    } else {
        match settings.whisper_model_language.as_str() {
            "en" => "en",
            _ => "multi",
        }
    };

    match backend {
        AsrBackend::WhisperCt2 => {
            let suffix = if language == "en" { "-en" } else { "" };
            Some(format!("whisper-ct2-{size}{suffix}"))
        }
        AsrBackend::WhisperOnnx => {
            let precision = match settings.whisper_precision.as_str() {
                "float" => "float",
                _ => "int8",
            };
            let lang_suffix = if language == "en" { "-en" } else { "" };
            Some(format!("whisper-onnx-{size}{lang_suffix}-{precision}"))
        }
        _ => None,
    }
}

fn parse_paste_shortcut(value: &str) -> PasteShortcut {
    match value {
        "ctrl-v" => PasteShortcut::CtrlV,
        "ctrl-shift-v" => PasteShortcut::CtrlShiftV,
        _ => PasteShortcut::CtrlShiftV,
    }
}

fn publish_hud_runtime_state(state: &AppState, hud_state: &str) {
    let overlay_enabled = state
        .settings_manager()
        .read_frontend()
        .map(|settings| settings.show_hud_overlay)
        .unwrap_or(false)
        && is_gnome_wayland_session();

    let path = match hud_runtime_state_path() {
        Some(path) => path,
        None => return,
    };

    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::debug!("failed creating runtime hud dir: {error}");
            return;
        }
    }

    let payload = serde_json::json!({
        "enabled": overlay_enabled,
        "state": hud_state,
        "pid": std::process::id(),
        "session_id": std::env::var("XDG_SESSION_ID").ok(),
    });

    if let Err(error) = std::fs::write(&path, payload.to_string()) {
        tracing::debug!("failed writing runtime hud state: {error}");
    }
}

fn hud_runtime_state_path() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .map(|base| base.join("openflow").join("hud-state.json"))
}

fn window_overlay_supported() -> bool {
    !is_gnome_wayland_session()
}

#[derive(Clone, Copy)]
struct OverlayMonitorTarget {
    origin_x: i32,
    origin_y: i32,
    width: u32,
    height: u32,
}

fn overlay_monitor_target_from_cursor(app: &AppHandle) -> Option<OverlayMonitorTarget> {
    let monitors = app.available_monitors().ok()?;
    if monitors.is_empty() {
        return None;
    }

    let cursor = app.cursor_position().ok();

    let selected = if let Some(cursor) = cursor {
        monitors.into_iter().find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            let left = position.x as f64;
            let top = position.y as f64;
            let right = left + size.width as f64;
            let bottom = top + size.height as f64;

            cursor.x >= left && cursor.x < right && cursor.y >= top && cursor.y < bottom
        })
    } else {
        None
    }
    .or_else(|| app.primary_monitor().ok().flatten());

    selected.map(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        OverlayMonitorTarget {
            origin_x: position.x,
            origin_y: position.y,
            width: size.width,
            height: size.height,
        }
    })
}

fn is_gnome_wayland_session() -> bool {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if !session.eq_ignore_ascii_case("wayland") {
        return false;
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_default();
    desktop
        .split(':')
        .any(|segment| segment.eq_ignore_ascii_case("gnome"))
}

/// Show the status overlay window positioned at the bottom center of the screen
fn show_status_overlay(app: &AppHandle, target_monitor: Option<OverlayMonitorTarget>) {
    tracing::info!("Showing status overlay window");

    // Try to get existing window first
    if let Some(window) = app.get_webview_window("status-overlay") {
        tracing::debug!("Found existing overlay window, showing it");
        // The overlay must never steal focus from the active input field.
        // `focused(false)` only controls initial focus state; some compositors may still
        // activate the window on show(). Make it explicitly non-focusable.
        let _ = window.set_focusable(false);
        let _ = window.set_visible_on_all_workspaces(true);
        let _ = window.set_always_on_top(true);
        if let Err(e) = window.show() {
            tracing::error!("Failed to show overlay window: {:?}", e);
        }
        // Defer positioning to avoid GTK assertion failures
        position_overlay_deferred(window, false, target_monitor);
    } else {
        tracing::info!("Creating new overlay window");
        // Create window if it doesn't exist (fallback)
        match WebviewWindowBuilder::new(
            app,
            "status-overlay",
            WebviewUrl::App("overlay.html".into()),
        )
        .title("")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible(false) // Start hidden to avoid GTK assertions during realization
        .skip_taskbar(true)
        .resizable(false)
        .inner_size(220.0, 180.0)
        .focused(false)
        .focusable(false)
        .visible_on_all_workspaces(true)
        .build()
        {
            Ok(window) => {
                tracing::info!("Overlay window created successfully");
                let _ = window.set_focusable(false);
                let _ = window.set_visible_on_all_workspaces(true);
                // Defer positioning and showing to avoid GTK assertion failures
                position_overlay_deferred(window, true, target_monitor);
            }
            Err(e) => {
                tracing::error!("Failed to create overlay window: {:?}", e);
            }
        }
    }
}

/// Position the overlay window after a small delay to ensure the GTK widget is realized
fn position_overlay_deferred(
    window: tauri::WebviewWindow,
    show_after: bool,
    target_monitor: Option<OverlayMonitorTarget>,
) {
    tauri::async_runtime::spawn(async move {
        // Wait for the window to be fully realized by GTK
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let monitor = target_monitor.or_else(|| {
            // Prefer current_monitor (where window is), fall back to primary.
            // This is only used when there is no cursor-derived monitor target.
            window
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| window.primary_monitor().ok().flatten())
                .map(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    OverlayMonitorTarget {
                        origin_x: position.x,
                        origin_y: position.y,
                        width: size.width,
                        height: size.height,
                    }
                })
        });

        if let Some(monitor) = monitor {
            let overlay_width = 220i32;
            let overlay_height = 180i32;
            let margin_bottom = 54i32;
            let x = monitor.origin_x + (monitor.width as i32 - overlay_width) / 2;
            let y = monitor.origin_y + monitor.height as i32 - overlay_height - margin_bottom;
            tracing::debug!("Positioning overlay at ({}, {})", x, y);
            let _ = window.set_position(PhysicalPosition::new(x, y));
        } else {
            tracing::warn!("No monitor available for overlay positioning");
        }

        // Show window after positioning if requested (for newly created windows)
        if show_after {
            let _ = window.show();
            // Ensure the underlying GTK/GDK window exists before we apply input shaping.
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        } else {
            // Existing overlay was shown immediately; still give GTK a beat.
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }

        // Keep the overlay non-interactive (click-through + never focusable).
        // NOTE: tao's CursorIgnoreEvents handler unwraps the underlying GdkWindow, so calling
        // this before the window is realized will panic. Only do this after show() + delay.
        let _ = window.set_focusable(false);
        let _ = window.set_visible_on_all_workspaces(true);
        let _ = window.set_always_on_top(true);
        let _ = window.set_ignore_cursor_events(true);

        // If the compositor still focused the overlay, immediately hide it.
        // This helps avoid breaking paste-to-active-app flows.
        if window.is_focused().unwrap_or(false) {
            tracing::warn!("Overlay window became focused; hiding to avoid stealing input focus");
            let _ = window.hide();
        }
    });
}

/// Hide the status overlay window
fn hide_status_overlay(app: &AppHandle) {
    tracing::info!("Hiding status overlay window");
    if let Some(window) = app.get_webview_window("status-overlay") {
        // Avoid poking GTK before the window is realized; it can emit warnings on Wayland.
        if !window.is_visible().unwrap_or(false) {
            return;
        }
        if let Err(e) = window.hide() {
            tracing::error!("Failed to hide overlay window: {:?}", e);
        }
    } else {
        tracing::warn!("Overlay window not found when trying to hide");
    }
}

fn format_asr_selection_label(settings: &crate::core::settings::FrontendSettings) -> String {
    if settings.asr_family == "whisper" {
        format!(
            "whisper:{}:{}:{}:{}",
            settings.whisper_backend,
            settings.whisper_model,
            settings.whisper_model_language,
            settings.whisper_precision
        )
    } else {
        "parakeet".to_string()
    }
}

fn default_asr_selection() -> AsrSelection {
    AsrSelection {
        asr_family: "parakeet".into(),
        whisper_backend: "ct2".into(),
        whisper_model: "small".into(),
        whisper_model_language: "multi".into(),
        whisper_precision: "int8".into(),
    }
}

async fn warmup_current_asr(app: &AppHandle, generation: u64) -> Result<()> {
    // Helper: only update state if this task is still current.
    let is_current = |app: &AppHandle| {
        let state = app.state::<AppState>();
        state.asr_warmup_generation.load(Ordering::SeqCst) == generation
    };

    // Attempt warmup for the currently-selected settings.
    let attempt = warmup_selected_asr(app, generation).await;
    if attempt.is_ok() {
        return Ok(());
    }

    // If the selection failed, fall back to last known-good.
    let (fallback, current) = {
        let state = app.state::<AppState>();
        let current_settings = state.settings_manager().read_frontend()?;
        let current = AsrSelection::from_frontend(&current_settings);
        let fallback = state
            .settings_manager()
            .read_last_known_good_asr()
            .unwrap_or_else(default_asr_selection);
        (fallback, current)
    };

    if fallback == current {
        let error = match &attempt {
            Ok(()) => "unknown warmup failure".to_string(),
            Err(err) => err.to_string(),
        };
        if is_current(app) {
            let state = app.state::<AppState>();
            let mut tracker = state.asr_warmup.lock();
            tracker.state = AsrWarmupState::Error;
            tracker.last_error = Some(error);
        }
        return attempt;
    }

    // Apply fallback selection to frontend settings and persist.
    {
        let state = app.state::<AppState>();
        let mut settings = state.settings_manager().read_frontend()?;
        fallback.apply_to_frontend(&mut settings);
        state.settings_manager().write_frontend(settings)?;
        if let Err(error) = state.reload_pipeline(app) {
            tracing::warn!("Failed to reload pipeline for fallback ASR selection: {error:?}");
        }

        if is_current(app) {
            let mut tracker = state.asr_warmup.lock();
            tracker.target_selection = Some(fallback.clone());
        }
    }

    // Warm the fallback selection.
    let result = warmup_selected_asr(app, generation).await;
    if let Err(error) = &result {
        if is_current(app) {
            let state = app.state::<AppState>();
            let mut tracker = state.asr_warmup.lock();
            tracker.state = AsrWarmupState::Error;
            tracker.last_error = Some(error.to_string());
        }
    }
    result
}

async fn warmup_selected_asr(app: &AppHandle, generation: u64) -> Result<()> {
    let is_current = |app: &AppHandle| {
        let state = app.state::<AppState>();
        state.asr_warmup_generation.load(Ordering::SeqCst) == generation
    };

    // Snapshot settings for this warmup.
    let settings = {
        let state = app.state::<AppState>();
        state.settings_manager().read_frontend()?
    };
    let selection = AsrSelection::from_frontend(&settings);

    ensure_asr_assets_ready(app, &settings, generation).await?;

    if !is_current(app) {
        return Ok(());
    }

    // Obtain the latest pipeline (it may be recreated after downloads complete).
    // After a model install, the download worker calls reload_pipeline(), which briefly
    // sets the pipeline to None. Wait for it to come back.
    let pipeline = {
        let mut waited_ms: u64 = 0;
        loop {
            if !is_current(app) {
                return Ok(());
            }

            let pipeline = {
                let state = app.state::<AppState>();
                let pipeline = state.pipeline.lock().as_ref().cloned();
                pipeline
            };

            if let Some(pipeline) = pipeline {
                if pipeline.asr_config().model_dir.is_some() {
                    break pipeline;
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            waited_ms = waited_ms.saturating_add(50);
            if waited_ms >= 10_000 {
                anyhow::bail!("pipeline ASR config not ready after model install")
            }
        }
    };

    // Heavy model initialization should run off the async runtime.
    let pipeline_clone = pipeline.clone();
    tokio::task::spawn_blocking(move || pipeline_clone.warmup_asr())
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))??;

    if !is_current(app) {
        return Ok(());
    }

    {
        let state = app.state::<AppState>();
        let mut tracker = state.asr_warmup.lock();
        tracker.state = AsrWarmupState::Ready;
        tracker.warmed_selection = Some(selection.clone());
        tracker.target_selection = Some(selection.clone());
        tracker.last_error = None;
        let _ = state
            .settings_manager()
            .write_last_known_good_asr(selection);
    }

    Ok(())
}

async fn ensure_asr_assets_ready(
    app: &AppHandle,
    settings: &crate::core::settings::FrontendSettings,
    generation: u64,
) -> Result<()> {
    let is_current = |app: &AppHandle| {
        let state = app.state::<AppState>();
        state.asr_warmup_generation.load(Ordering::SeqCst) == generation
    };

    let backend = parse_asr_backend(settings);

    // If already installed, we're done.
    {
        let state = app.state::<AppState>();
        if state.resolve_asr_model_dir(settings, &backend).is_some() {
            return Ok(());
        }
    }

    let asset_name = {
        let state = app.state::<AppState>();
        state.required_asr_asset_name(settings, &backend)
    }
    .ok_or_else(|| anyhow!("no ASR model asset found for selection"))?;

    let mut queued = false;
    loop {
        if !is_current(app) {
            return Ok(());
        }

        // Check if the model dir is now available.
        {
            let state = app.state::<AppState>();
            if state.resolve_asr_model_dir(settings, &backend).is_some() {
                return Ok(());
            }
        }

        // Check model manager status to decide whether to queue or fail.
        let status = {
            let state = app.state::<AppState>();
            let guard = state
                .models
                .lock()
                .map_err(|err| anyhow!(err.to_string()))?;
            guard
                .asset_by_name(&asset_name)
                .map(|asset| asset.status.clone())
        };

        match status {
            Some(ModelStatus::Installed) => return Ok(()),
            Some(ModelStatus::Error(message)) => {
                anyhow::bail!("model download failed: {message}")
            }
            Some(ModelStatus::NotInstalled) => {
                if !queued {
                    let state = app.state::<AppState>();
                    if let Err(error) = state.queue_model_download(app, &asset_name) {
                        tracing::warn!("Failed to queue ASR model download: {error:?}");
                    } else {
                        queued = true;
                    }
                }
            }
            Some(ModelStatus::Downloading { .. }) => {
                // Wait.
            }
            None => {
                // Asset might not exist in manifest; nothing we can do.
                anyhow::bail!("unknown ASR model asset: {asset_name}")
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

impl AppState {
    fn required_asr_asset_name(
        &self,
        settings: &crate::core::settings::FrontendSettings,
        backend: &AsrBackend,
    ) -> Option<String> {
        match *backend {
            AsrBackend::WhisperOnnx | AsrBackend::WhisperCt2 => {
                resolve_whisper_asset_name(settings, backend)
            }
            AsrBackend::Parakeet => {
                let guard = self.models.lock().ok()?;
                guard
                    .primary_asset(&ModelKind::Parakeet)
                    .map(|asset| asset.name.clone())
            }
        }
    }
}
