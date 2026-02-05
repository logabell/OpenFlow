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
    pub asr_backend: String,
    pub paste_shortcut: String,
    pub language: String,
    pub auto_detect_language: bool,
    pub autoclean_mode: String,
    pub polish_model_ready: bool,
    pub debug_transcripts: bool,
    pub audio_device_id: Option<String>,
    pub vad_sensitivity: String,
}

/// Default hotkey for push-to-talk mode (Alt+Shift+A avoids IME conflicts on Linux)
pub const DEFAULT_PUSH_TO_TALK_HOTKEY: &str = "Alt+Shift+A";
/// Default hotkey for toggle-to-talk mode
pub const DEFAULT_TOGGLE_TO_TALK_HOTKEY: &str = "Alt+Shift+S";

impl Default for FrontendSettings {
    fn default() -> Self {
        Self {
            hotkey_mode: "hold".into(),
            push_to_talk_hotkey: DEFAULT_PUSH_TO_TALK_HOTKEY.into(),
            toggle_to_talk_hotkey: DEFAULT_TOGGLE_TO_TALK_HOTKEY.into(),
            hud_theme: "system".into(),
            show_overlay_on_wayland: false,
            asr_backend: "parakeet".into(),
            paste_shortcut: "ctrl-shift-v".into(),
            language: "auto".into(),
            auto_detect_language: true,
            autoclean_mode: "fast".into(),
            polish_model_ready: false,
            debug_transcripts: false,
            audio_device_id: None,
            vad_sensitivity: "medium".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PersistedSettings {
    frontend: FrontendSettings,
    debug_transcripts_until: Option<OffsetDateTime>,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            frontend: FrontendSettings::default(),
            debug_transcripts_until: None,
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
        Ok(guard.frontend.clone())
    }

    pub fn write_frontend(&self, settings: FrontendSettings) -> Result<()> {
        let mut guard = self.inner.write();

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

    pub fn set_polish_ready(&self, ready: bool) -> Result<()> {
        let mut guard = self.inner.write();
        if guard.frontend.polish_model_ready == ready {
            return Ok(());
        }
        guard.frontend.polish_model_ready = ready;
        persist_settings(self.path.as_path(), &guard)
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
