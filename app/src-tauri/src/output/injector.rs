use std::io::Write;
use std::process::{Command, Stdio};

#[cfg(debug_assertions)]
use crate::output::logs;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[cfg(target_os = "linux")]
use crate::output::uinput;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputAction {
    Paste,
    Copy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PasteShortcut {
    CtrlV,
    CtrlShiftV,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteFailureStep {
    ClipboardWrite,
    KeyInject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteFailureKind {
    Failed,
    Unconfirmed,
}

impl PasteFailureStep {
    pub fn as_str(&self) -> &'static str {
        match self {
            PasteFailureStep::ClipboardWrite => "wl-copy",
            PasteFailureStep::KeyInject => "uinput",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PasteFailure {
    pub step: PasteFailureStep,
    pub kind: PasteFailureKind,
    pub message: String,
    pub transcript_on_clipboard: bool,
}

impl std::fmt::Display for PasteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.step.as_str(), self.message)
    }
}

impl std::error::Error for PasteFailure {}

#[derive(Debug, Clone)]
pub enum OutputInjectionError {
    Paste(PasteFailure),
    Copy(String),
}

impl std::fmt::Display for OutputInjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputInjectionError::Paste(err) => write!(f, "{err}"),
            OutputInjectionError::Copy(message) => write!(f, "wl-copy: {message}"),
        }
    }
}

impl std::error::Error for OutputInjectionError {}

impl Default for PasteShortcut {
    fn default() -> Self {
        PasteShortcut::CtrlShiftV
    }
}

pub struct OutputInjector {
    paste_shortcut: std::sync::Mutex<PasteShortcut>,
}

impl OutputInjector {
    pub fn new() -> Self {
        Self {
            paste_shortcut: std::sync::Mutex::new(PasteShortcut::default()),
        }
    }

    pub fn set_paste_shortcut(&self, shortcut: PasteShortcut) {
        if let Ok(mut guard) = self.paste_shortcut.lock() {
            *guard = shortcut;
        }
    }

    pub fn current_paste_shortcut(&self) -> PasteShortcut {
        self.paste_shortcut
            .lock()
            .map(|guard| *guard)
            .unwrap_or_default()
    }

    pub fn inject(&self, text: &str, action: OutputAction) -> Result<(), OutputInjectionError> {
        let shortcut = self
            .paste_shortcut
            .lock()
            .map(|guard| *guard)
            .unwrap_or_default();
        match action {
            OutputAction::Paste => match paste_text(text, shortcut) {
                Ok(()) => {
                    #[cfg(debug_assertions)]
                    logs::push_log(format!("Paste -> {}", text));
                    Ok(())
                }
                Err(error) => {
                    match error.kind {
                        PasteFailureKind::Unconfirmed => {
                            warn!("Paste unconfirmed: {error}");
                        }
                        PasteFailureKind::Failed => {
                            warn!("Paste failed: {error}");
                        }
                    }
                    #[cfg(debug_assertions)]
                    logs::push_log(format!("Paste {} ({})", error.kind.as_str(), error));
                    Err(OutputInjectionError::Paste(error))
                }
            },
            OutputAction::Copy => set_clipboard_text(text)
                .map_err(|error| {
                    warn!("Copy failed: {error}");
                    OutputInjectionError::Copy(error.to_string())
                })
                .map(|_| ()),
        }
    }
}

fn paste_text(text: &str, shortcut: PasteShortcut) -> Result<(), PasteFailure> {
    #[cfg(target_os = "linux")]
    {
        use std::thread::sleep;
        use std::time::Duration;

        let previous = snapshot_clipboard().ok().flatten();

        // Ensure transcript is available on the Wayland clipboard before we inject the paste.
        set_clipboard_text(text).map_err(|err| PasteFailure {
            step: PasteFailureStep::ClipboardWrite,
            kind: PasteFailureKind::Failed,
            message: err.to_string(),
            transcript_on_clipboard: false,
        })?;

        if !wait_for_clipboard_equals(text.as_bytes(), Duration::from_millis(250)) {
            return Err(PasteFailure {
                step: PasteFailureStep::ClipboardWrite,
                kind: PasteFailureKind::Unconfirmed,
                message: "Transcript not observed on clipboard before paste; transcript left on clipboard.".to_string(),
                transcript_on_clipboard: true,
            });
        }

        if let Err(error) = uinput::send_paste(shortcut) {
            // Keep transcript on the clipboard so the user can paste manually.
            let _ = set_clipboard_text(text);
            return Err(PasteFailure {
                step: PasteFailureStep::KeyInject,
                kind: PasteFailureKind::Failed,
                message: error.to_string(),
                transcript_on_clipboard: true,
            });
        }

        // Hold the transcript as the clipboard selection long enough for the target app
        // to request it. Clipboard managers may probe immediately; we must not restore early.
        sleep(Duration::from_millis(650));

        let Some(previous) = previous else {
            return Err(PasteFailure {
                step: PasteFailureStep::ClipboardWrite,
                kind: PasteFailureKind::Unconfirmed,
                message:
                    "Previous clipboard could not be snapshotted; transcript left on clipboard."
                        .to_string(),
                transcript_on_clipboard: true,
            });
        };

        // If the clipboard changed while we were holding the transcript (e.g. user copied
        // something), do not overwrite it.
        if !clipboard_equals(text.as_bytes()) {
            return Err(PasteFailure {
                step: PasteFailureStep::ClipboardWrite,
                kind: PasteFailureKind::Unconfirmed,
                message: "Clipboard changed during paste window; not restoring previous clipboard."
                    .to_string(),
                transcript_on_clipboard: false,
            });
        }

        restore_clipboard(previous).map_err(|err| PasteFailure {
            step: PasteFailureStep::ClipboardWrite,
            kind: PasteFailureKind::Unconfirmed,
            message: format!("Failed to restore clipboard: {err}"),
            transcript_on_clipboard: true,
        })?;

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        warn!("Paste requested on unsupported platform: {}", text);
        Ok(())
    }
}

impl PasteFailureKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PasteFailureKind::Failed => "failed",
            PasteFailureKind::Unconfirmed => "unconfirmed",
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct ClipboardSnapshot {
    mime: String,
    data: Vec<u8>,
}

#[cfg(target_os = "linux")]
fn snapshot_clipboard() -> anyhow::Result<Option<ClipboardSnapshot>> {
    let types = list_clipboard_types()?;
    if types.is_empty() {
        return Ok(None);
    }

    let chosen = choose_preferred_type(&types).unwrap_or_else(|| types[0].as_str());
    let output = Command::new(resolve_binary("wl-paste"))
        .args(["--type", chosen, "--no-newline"])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }

    // Avoid unbounded memory usage.
    const MAX_BYTES: usize = 8 * 1024 * 1024;
    if output.stdout.len() > MAX_BYTES {
        return Ok(None);
    }

    Ok(Some(ClipboardSnapshot {
        mime: chosen.to_string(),
        data: output.stdout,
    }))
}

#[cfg(target_os = "linux")]
fn list_clipboard_types() -> anyhow::Result<Vec<String>> {
    let output = Command::new(resolve_binary("wl-paste"))
        .args(["--list-types"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("wl-paste --list-types failed with status {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect())
}

#[cfg(target_os = "linux")]
fn choose_preferred_type(types: &[String]) -> Option<&str> {
    for candidate in ["text/plain;charset=utf-8", "text/plain"] {
        if types.iter().any(|t| t == candidate) {
            return Some(candidate);
        }
    }

    types
        .iter()
        .find(|t| t.starts_with("text/"))
        .map(|t| t.as_str())
}

#[cfg(target_os = "linux")]
fn set_clipboard_text(text: &str) -> anyhow::Result<()> {
    let mut child = Command::new(resolve_binary("wl-copy"))
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("wl-copy failed with status {status}");
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn set_clipboard_text(_text: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn restore_clipboard(snapshot: ClipboardSnapshot) -> anyhow::Result<()> {
    let mut child = Command::new(resolve_binary("wl-copy"))
        .args(["--type", snapshot.mime.as_str()])
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(&snapshot.data)?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("wl-copy failed with status {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn clipboard_equals(expected: &[u8]) -> bool {
    Command::new(resolve_binary("wl-paste"))
        .args(["--no-newline"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| out.stdout == expected)
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn wait_for_clipboard_equals(expected: &[u8], timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        if clipboard_equals(expected) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn resolve_binary(binary: &str) -> std::ffi::OsString {
    for dir in ["/usr/bin", "/usr/local/bin", "/bin"] {
        let candidate = std::path::Path::new(dir).join(binary);
        if candidate.is_file() {
            return candidate.into_os_string();
        }
    }
    std::ffi::OsString::from(binary)
}
