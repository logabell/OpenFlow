use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

const CONFIG_FILE: &str = "config.json";
const DEBUG_TRANSCRIPT_TTL: Duration = Duration::hours(24);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FrontendSettings {
    pub hotkey_mode: String,
    pub push_to_talk_hotkey: String,
    pub toggle_to_talk_hotkey: String,
    pub hud_theme: String,
    pub show_overlay_on_wayland: bool,
    pub asr_family: String,
    pub whisper_backend: String,
    pub whisper_model: String,
    pub whisper_model_language: String,
    pub whisper_precision: String,
    pub paste_shortcut: String,
    pub language: String,
    pub auto_detect_language: bool,
    pub autoclean_mode: String,
    pub debug_transcripts: bool,
    pub audio_device_id: Option<String>,
    pub vad_sensitivity: String,
    #[serde(default, skip_serializing)]
    #[serde(rename = "asrBackend")]
    pub legacy_asr_backend: Option<String>,
}

/// Persisted snapshot of the ASR model selection.
///
/// This is intentionally a small subset of FrontendSettings so we can fall back
/// to a previously known-good model without overwriting unrelated settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsrSelection {
    pub asr_family: String,
    pub whisper_backend: String,
    pub whisper_model: String,
    pub whisper_model_language: String,
    pub whisper_precision: String,
}

impl AsrSelection {
    pub fn from_frontend(settings: &FrontendSettings) -> Self {
        Self {
            asr_family: settings.asr_family.clone(),
            whisper_backend: settings.whisper_backend.clone(),
            whisper_model: settings.whisper_model.clone(),
            whisper_model_language: settings.whisper_model_language.clone(),
            whisper_precision: settings.whisper_precision.clone(),
        }
    }

    pub fn apply_to_frontend(&self, settings: &mut FrontendSettings) {
        settings.asr_family = self.asr_family.clone();
        settings.whisper_backend = self.whisper_backend.clone();
        settings.whisper_model = self.whisper_model.clone();
        settings.whisper_model_language = self.whisper_model_language.clone();
        settings.whisper_precision = self.whisper_precision.clone();
    }
}

// Defaults are intentionally OS-specific.
// - Linux uses single-key hotkeys (evdev backend handles these reliably).
// - Non-Linux uses chord-style defaults that work well with global shortcut backends.
#[cfg(target_os = "linux")]
pub const DEFAULT_PUSH_TO_TALK_HOTKEY: &str = "RightAlt";
#[cfg(target_os = "linux")]
pub const DEFAULT_TOGGLE_TO_TALK_HOTKEY: &str = "RightAlt";

#[cfg(not(target_os = "linux"))]
pub const DEFAULT_PUSH_TO_TALK_HOTKEY: &str = "Ctrl+Space";
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_TOGGLE_TO_TALK_HOTKEY: &str = "Ctrl+Shift+Space";

impl Default for FrontendSettings {
    fn default() -> Self {
        Self {
            hotkey_mode: "hold".into(),
            push_to_talk_hotkey: DEFAULT_PUSH_TO_TALK_HOTKEY.into(),
            toggle_to_talk_hotkey: DEFAULT_TOGGLE_TO_TALK_HOTKEY.into(),
            hud_theme: "system".into(),
            show_overlay_on_wayland: false,
            asr_family: "parakeet".into(),
            whisper_backend: "ct2".into(),
            whisper_model: "small".into(),
            whisper_model_language: "multi".into(),
            whisper_precision: "int8".into(),
            paste_shortcut: "ctrl-shift-v".into(),
            language: "auto".into(),
            auto_detect_language: true,
            autoclean_mode: "fast".into(),
            debug_transcripts: false,
            audio_device_id: None,
            vad_sensitivity: "medium".into(),
            legacy_asr_backend: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PersistedSettings {
    frontend: FrontendSettings,
    debug_transcripts_until: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_known_good_asr: Option<AsrSelection>,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            frontend: FrontendSettings::default(),
            debug_transcripts_until: None,
            last_known_good_asr: None,
        }
    }
}

pub struct SettingsManager {
    path: PathBuf,
    inner: RwLock<PersistedSettings>,
}

impl SettingsManager {
    pub fn new() -> Self {
        let config_path = resolve_config_path().expect("failed to resolve config directory");
        let persisted = load_settings(&config_path).unwrap_or_default();
        Self {
            path: config_path,
            inner: RwLock::new(persisted),
        }
    }

    pub fn read_frontend(&self) -> Result<FrontendSettings> {
        let mut guard = self.inner.write();
        maybe_expire_debug_transcripts(&mut guard);
        migrate_frontend_settings(&mut guard.frontend);
        Ok(guard.frontend.clone())
    }

    pub fn write_frontend(&self, settings: FrontendSettings) -> Result<()> {
        let mut guard = self.inner.write();
        let mut settings = settings;
        migrate_frontend_settings(&mut settings);

        if settings.debug_transcripts {
            guard.debug_transcripts_until = Some(OffsetDateTime::now_utc() + DEBUG_TRANSCRIPT_TTL);
        } else {
            guard.debug_transcripts_until = None;
        }

        guard.frontend = settings.clone();
        guard.frontend.debug_transcripts = settings.debug_transcripts;

        persist_settings(self.path.as_path(), &guard)?;
        Ok(())
    }

    pub fn read_last_known_good_asr(&self) -> Option<AsrSelection> {
        let guard = self.inner.read();
        guard.last_known_good_asr.clone()
    }

    pub fn write_last_known_good_asr(&self, selection: AsrSelection) -> Result<()> {
        let mut guard = self.inner.write();
        guard.last_known_good_asr = Some(selection);
        persist_settings(self.path.as_path(), &guard)?;
        Ok(())
    }

    /// Returns the current active hotkey based on the hotkey mode setting.
    pub fn current_hotkey(&self) -> String {
        let guard = self.inner.read();
        match guard.frontend.hotkey_mode.as_str() {
            "toggle" => guard.frontend.toggle_to_talk_hotkey.clone(),
            _ => guard.frontend.push_to_talk_hotkey.clone(),
        }
    }
}

fn resolve_config_path() -> Result<PathBuf> {
    let project_dirs = ProjectDirs::from("com", "PushToTalk", "PushToTalk")
        .context("missing project directories")?;
    let dir = project_dirs.config_dir();
    fs::create_dir_all(dir).context("creating config directory failed")?;
    Ok(dir.join(CONFIG_FILE))
}

fn load_settings(path: &Path) -> Result<PersistedSettings> {
    if !path.exists() {
        return Ok(PersistedSettings::default());
    }
    let bytes = fs::read(path).with_context(|| format!("failed reading {path:?}"))?;
    let mut parsed: PersistedSettings =
        serde_json::from_slice(&bytes).context("config json could not be parsed")?;
    maybe_expire_debug_transcripts(&mut parsed);
    Ok(parsed)
}

fn persist_settings(path: &Path, settings: &PersistedSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {parent:?}"))?;
    }
    let serialized =
        serde_json::to_vec_pretty(settings).context("serialize settings to json failed")?;
    fs::write(path, serialized).with_context(|| format!("write settings to {path:?}"))?;
    Ok(())
}

fn maybe_expire_debug_transcripts(settings: &mut PersistedSettings) {
    if let Some(expires_at) = settings.debug_transcripts_until {
        if OffsetDateTime::now_utc() > expires_at {
            settings.frontend.debug_transcripts = false;
            settings.debug_transcripts_until = None;
        } else {
            settings.frontend.debug_transcripts = true;
        }
    } else {
        settings.frontend.debug_transcripts = false;
    }
}

fn migrate_frontend_settings(settings: &mut FrontendSettings) {
    // Keep hotkeys non-empty.
    if settings.push_to_talk_hotkey.trim().is_empty() {
        settings.push_to_talk_hotkey = DEFAULT_PUSH_TO_TALK_HOTKEY.into();
    }
    if settings.toggle_to_talk_hotkey.trim().is_empty() {
        settings.toggle_to_talk_hotkey = DEFAULT_TOGGLE_TO_TALK_HOTKEY.into();
    }

    // Linux: migrate legacy defaults to the newer single-key default.
    // Only rewrite when the user is still on the old shipped defaults.
    if cfg!(target_os = "linux") {
        const LEGACY_LINUX_PUSH_TO_TALK: &str = "Alt+Shift+A";
        const LEGACY_LINUX_TOGGLE_TO_TALK: &str = "Alt+Shift+S";

        if settings.push_to_talk_hotkey == LEGACY_LINUX_PUSH_TO_TALK {
            settings.push_to_talk_hotkey = DEFAULT_PUSH_TO_TALK_HOTKEY.into();
        }
        if settings.toggle_to_talk_hotkey == LEGACY_LINUX_TOGGLE_TO_TALK {
            settings.toggle_to_talk_hotkey = DEFAULT_TOGGLE_TO_TALK_HOTKEY.into();
        }
    }

    if let Some(legacy) = settings.legacy_asr_backend.take() {
        match legacy.as_str() {
            "whisper" => {
                settings.asr_family = "whisper".into();
                settings.whisper_backend = "onnx".into();
            }
            "parakeet" => {
                settings.asr_family = "parakeet".into();
            }
            _ => {
                settings.asr_family = "parakeet".into();
            }
        }
    }

    if settings.asr_family.is_empty() {
        settings.asr_family = "parakeet".into();
    }
    if settings.whisper_backend.is_empty() {
        settings.whisper_backend = "ct2".into();
    }
    if settings.whisper_model.is_empty() {
        settings.whisper_model = "small".into();
    }
    if settings.whisper_model_language.is_empty() {
        settings.whisper_model_language = "multi".into();
    }
    if settings.whisper_precision.is_empty() {
        settings.whisper_precision = "int8".into();
    }

    if settings.autoclean_mode == "polish" {
        settings.autoclean_mode = "fast".into();
    }

    if matches!(
        settings.whisper_model.as_str(),
        "large-v3" | "large-v3-turbo"
    ) {
        settings.whisper_model_language = "multi".into();
    }
}
