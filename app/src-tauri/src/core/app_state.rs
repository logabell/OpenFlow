use std::sync::{Arc, Mutex as StdMutex};

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
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindowBuilder};
use tauri::WebviewUrl;
use tracing::{debug, warn};

use super::pipeline::{OutputMode, SpeechPipeline};
use super::settings::SettingsManager;

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
}

impl AppState {
    pub fn new() -> Self {
        let models = ModelManager::new().expect("failed to initialize model manager");
        Self {
            settings: Arc::new(SettingsManager::new()),
            pipeline: Arc::new(Mutex::new(None)),
            session: Arc::new(Mutex::new(SessionState::Idle)),
            models: Arc::new(StdMutex::new(models)),
            downloads: Arc::new(Mutex::new(None)),
        }
    }

    pub fn settings_manager(&self) -> Arc<SettingsManager> {
        self.settings.clone()
    }

    pub fn model_manager(&self) -> Arc<StdMutex<ModelManager>> {
        self.models.clone()
    }

    pub fn start_session(&self, app: &AppHandle) {
        let show_overlay = {
            #[cfg(target_os = "linux")]
            {
                if is_wayland_session() {
                    self.settings_manager()
                        .read_frontend()
                        .map(|settings| settings.show_overlay_on_wayland)
                        .unwrap_or(false)
                } else {
                    true
                }
            }

            #[cfg(not(target_os = "linux"))]
            {
                true
            }
        };

        self.start_session_with_overlay(app, show_overlay);
    }

    pub fn start_session_with_overlay(&self, app: &AppHandle, show_overlay: bool) {
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

        if show_overlay {
            show_status_overlay(app);
        } else if app.get_webview_window("status-overlay").is_some() {
            // Make sure a previously-shown overlay can't steal focus/cancel input
            // while using debug hold-to-talk.
            hide_status_overlay(app);
        }

        events::emit_hud_state(app, "listening");
    }

    pub fn mark_processing(&self, app: &AppHandle) {
        let mut guard = self.session.lock();
        if *guard != SessionState::Listening {
            return;
        }
        *guard = SessionState::Processing;
        events::emit_hud_state(app, "processing");
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
            events::emit_hud_state(app, "processing");
        }

        // Clone the pipeline handle so we can finalize without holding the mutex.
        let pipeline = { self.pipeline.lock().as_ref().cloned() };
        let app_handle = app.clone();
        let session = self.session.clone();

        // If we weren't in an active session, still force-hide the overlay immediately.
        if matches!(previous, SessionState::Idle) {
            hide_status_overlay(app);
            events::emit_hud_state(app, "idle");
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

            hide_status_overlay(&app_handle);
            events::emit_hud_state(&app_handle, "idle");
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
        let pipeline =
            SpeechPipeline::new(app.clone(), audio_config, vad_config.clone(), desired_asr_config);
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

        // Auto-download default models if they're not installed
        self.auto_download_default_models(app);

        Ok(())
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

            let parakeet_asset = guard.primary_asset(&ModelKind::Parakeet).map(|a| a.name.clone());
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

        let (language, auto_language_detect) = if settings.asr_family == "whisper"
            && settings.whisper_model_language == "en"
        {
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

        self.models
            .lock()
            .ok()
            .and_then(|guard| {
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

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    xdg_session_type == "wayland" || !wayland_display.is_empty()
}

/// Show the status overlay window positioned at the bottom center of the screen
fn show_status_overlay(app: &AppHandle) {
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
        position_overlay_deferred(window, false);
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
        .visible(false)  // Start hidden to avoid GTK assertions during realization
        .skip_taskbar(true)
        .resizable(false)
        .inner_size(200.0, 200.0)
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
                position_overlay_deferred(window, true);
            }
            Err(e) => {
                tracing::error!("Failed to create overlay window: {:?}", e);
            }
        }
    }
}

/// Position the overlay window after a small delay to ensure the GTK widget is realized
fn position_overlay_deferred(window: tauri::WebviewWindow, show_after: bool) {
    tauri::async_runtime::spawn(async move {
        // Wait for the window to be fully realized by GTK
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Prefer current_monitor (where window is), fall back to primary
        let monitor = window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten());

        if let Some(monitor) = monitor {
            let size = monitor.size();
            let x = (size.width as i32 / 2) - 100; // 100 = half of 200px window width
            let y = size.height as i32 - 250; // 250px from bottom
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
