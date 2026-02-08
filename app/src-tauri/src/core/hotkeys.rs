use parking_lot::RwLock;
use tauri::{AppHandle, Emitter};
use tauri::Manager;
use tracing::{info, warn};

use crate::core::app_state::AppState;
use crate::core::events;
use crate::core::settings::DEFAULT_PUSH_TO_TALK_HOTKEY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyState {
    Pressed,
    Released,
}

/// Tracks the currently registered hotkey so we can unregister it when changing.
static CURRENT_HOTKEY: RwLock<Option<String>> = RwLock::new(None);

/// Register the hotkey based on current settings.
/// This will unregister any previously registered hotkey first.
pub async fn register(app: &AppHandle) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<AppState>() {
        state.complete_session(app);
    }

    let shortcut = get_current_hotkey(app);
    register_shortcut(app, &shortcut).await
}

/// Register a specific hotkey shortcut.
pub async fn register_shortcut(app: &AppHandle, shortcut: &str) -> tauri::Result<()> {
    unregister_current(app).await?;

    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    info!(
        "Registering hotkey: {} (session_type={})",
        shortcut, session_type
    );

    register_evdev_shortcut(app, shortcut)?;
    set_current_hotkey(shortcut);
    let _ = app.emit("hotkey-backend", "evdev");
    if let Some(state) = app.try_state::<AppState>() {
        state.set_hud_state(app, "idle");
    } else {
        events::emit_hud_state(app, "idle");
    }
    app.emit("hotkey-registered", shortcut)?;
    Ok(())
}

fn handle_hotkey_state(app: &AppHandle, state: HotkeyState) {
    let app_handle = app.clone();
    let state_handle = app_handle.state::<AppState>();
    let mode = state_handle.hotkey_mode();

    let _ = app_handle.emit(
        "hotkey-event",
        match state {
            HotkeyState::Pressed => "pressed",
            HotkeyState::Released => "released",
        },
    );

    match mode.as_str() {
        "toggle" => {
            if matches!(state, HotkeyState::Pressed) {
                if state_handle.is_listening() {
                    state_handle.mark_processing(&app_handle);
                    state_handle.complete_session(&app_handle);
                } else {
                    state_handle.start_session(&app_handle);
                }
            }
        }
        _ => match state {
            HotkeyState::Pressed => {
                state_handle.start_session(&app_handle);
            }
            HotkeyState::Released => {
                if state_handle.is_listening() {
                    state_handle.mark_processing(&app_handle);
                }
                state_handle.complete_session(&app_handle);
            }
        },
    }
}

/// Unregister the currently registered hotkey (if any).
async fn unregister_current(_app: &AppHandle) -> tauri::Result<()> {
    let current = { CURRENT_HOTKEY.read().clone() };
    if current.is_some() {
        stop_evdev_listener();
    }

    {
        let mut guard = CURRENT_HOTKEY.write();
        *guard = None;
    }

    Ok(())
}

fn set_current_hotkey(shortcut: &str) {
    let mut current = CURRENT_HOTKEY.write();
    *current = Some(shortcut.to_string());
}

/// Get the current hotkey from settings based on the active mode.
fn get_current_hotkey(app: &AppHandle) -> String {
    if let Some(state) = app.try_state::<AppState>() {
        state.settings_manager().current_hotkey()
    } else {
        DEFAULT_PUSH_TO_TALK_HOTKEY.to_string()
    }
}

/// Unregister all hotkeys.
pub async fn unregister(app: &AppHandle) -> tauri::Result<()> {
    let current = { CURRENT_HOTKEY.read().clone() };
    unregister_current(app).await?;

    if let Some(shortcut) = current {
        app.emit("hotkey-unregistered", shortcut)?;
    }
    Ok(())
}

/// Re-register the hotkey after settings have changed.
/// This should be called whenever the hotkey mode or hotkey bindings change.
pub async fn reregister(app: &AppHandle) -> tauri::Result<()> {
    let new_shortcut = get_current_hotkey(app);
    let current = { CURRENT_HOTKEY.read().clone() };

    if current.as_deref() != Some(new_shortcut.as_str()) {
        info!(
            "Hotkey changed from {:?} to {}, re-registering",
            current, new_shortcut
        );
        register_shortcut(app, &new_shortcut).await?;
    }

    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Linux evdev backend
// -------------------------------------------------------------------------------------------------

mod linux_evdev {
    use super::{handle_hotkey_state, HotkeyState};
    use crate::output::uinput::VIRTUAL_KEYBOARD_NAME;
    use evdev::{Device, InputEventKind, Key};
    use inotify::{Inotify, WatchMask};
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::os::unix::io::AsRawFd;
    use std::path::PathBuf;
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::thread;
    use std::time::{Duration, Instant};
    use tauri::AppHandle;
    use tracing::{debug, info, warn};

    use libc::{fcntl, F_GETFL, F_SETFL, O_NONBLOCK};

    #[derive(Debug, Clone, Copy)]
    struct Modifiers {
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct HotkeySpec {
        key: Key,
        modifiers: Modifiers,
    }

    pub(super) struct EvdevListener {
        stop_tx: Sender<()>,
        thread: thread::JoinHandle<()>,
    }

    static EVDEV_LISTENER: parking_lot::RwLock<Option<EvdevListener>> =
        parking_lot::RwLock::new(None);

    pub(super) fn start(app: &AppHandle, shortcut: &str) -> anyhow::Result<()> {
        stop();
        let spec = parse_hotkey(shortcut)?;
        let app_handle = app.clone();

        let (stop_tx, stop_rx) = channel();
        let thread = thread::Builder::new()
            .name("evdev-hotkeys".to_string())
            .spawn(move || {
                if let Err(error) = run_loop(app_handle, spec, stop_rx) {
                    warn!("evdev hotkey listener stopped: {error:?}");
                }
            })?;

        *EVDEV_LISTENER.write() = Some(EvdevListener { stop_tx, thread });
        Ok(())
    }

    pub(super) fn stop() {
        let listener = EVDEV_LISTENER.write().take();
        if let Some(listener) = listener {
            let _ = listener.stop_tx.send(());
            let _ = listener.thread.join();
        }
    }

    pub(super) fn stop_from_parent() {
        stop();
    }

    fn parse_hotkey(input: &str) -> anyhow::Result<HotkeySpec> {
        let parts: Vec<&str> = input
            .split('+')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();

        if parts.is_empty() {
            anyhow::bail!("hotkey is empty");
        }

        let (mods, key_str) = if parts.len() == 1 {
            (Vec::new(), parts[0])
        } else {
            (parts[..parts.len() - 1].to_vec(), parts[parts.len() - 1])
        };

        let mut modifiers = Modifiers {
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        };

        for m in mods {
            match m {
                "Ctrl" | "Control" => modifiers.ctrl = true,
                "Alt" => modifiers.alt = true,
                "Shift" => modifiers.shift = true,
                "Meta" | "Super" | "Command" | "Logo" => modifiers.meta = true,
                _ => {}
            }
        }

        let key = parse_key(key_str)?;
        Ok(HotkeySpec { key, modifiers })
    }

    fn parse_key(key: &str) -> anyhow::Result<Key> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            anyhow::bail!("missing hotkey key");
        }

        let upper = trimmed.to_ascii_uppercase();
        let upper = upper.replace(' ', "");

        let mapped = match upper.as_str() {
            "SPACE" => Key::KEY_SPACE,
            "ENTER" | "RETURN" => Key::KEY_ENTER,
            "ESC" | "ESCAPE" => Key::KEY_ESC,
            "ARROWUP" | "UP" => Key::KEY_UP,
            "ARROWDOWN" | "DOWN" => Key::KEY_DOWN,
            "ARROWLEFT" | "LEFT" => Key::KEY_LEFT,
            "ARROWRIGHT" | "RIGHT" => Key::KEY_RIGHT,
            "TAB" => Key::KEY_TAB,
            "BACKSPACE" => Key::KEY_BACKSPACE,

            "RIGHTALT" | "ALTRIGHT" => Key::KEY_RIGHTALT,
            "LEFTALT" | "ALTLEFT" => Key::KEY_LEFTALT,
            "RIGHTCTRL" | "CTRLRIGHT" | "CONTROLRIGHT" => Key::KEY_RIGHTCTRL,
            "LEFTCTRL" | "CTRLLEFT" | "CONTROLLEFT" => Key::KEY_LEFTCTRL,
            "RIGHTSHIFT" | "SHIFTRIGHT" => Key::KEY_RIGHTSHIFT,
            "LEFTSHIFT" | "SHIFTLEFT" => Key::KEY_LEFTSHIFT,
            "RIGHTMETA" | "METARIGHT" | "SUPERRIGHT" => Key::KEY_RIGHTMETA,
            "LEFTMETA" | "METALEFT" | "SUPERLEFT" => Key::KEY_LEFTMETA,

            "SCROLLLOCK" => Key::KEY_SCROLLLOCK,
            "PAUSE" => Key::KEY_PAUSE,
            "CAPSLOCK" => Key::KEY_CAPSLOCK,
            "NUMLOCK" => Key::KEY_NUMLOCK,
            "INSERT" => Key::KEY_INSERT,
            "HOME" => Key::KEY_HOME,
            "END" => Key::KEY_END,
            "PAGEUP" => Key::KEY_PAGEUP,
            "PAGEDOWN" => Key::KEY_PAGEDOWN,
            "DELETE" => Key::KEY_DELETE,

            _ => {
                // Function keys
                if let Some(num) = upper.strip_prefix('F') {
                    if let Ok(n) = num.parse::<u8>() {
                        let key = match n {
                            1 => Some(Key::KEY_F1),
                            2 => Some(Key::KEY_F2),
                            3 => Some(Key::KEY_F3),
                            4 => Some(Key::KEY_F4),
                            5 => Some(Key::KEY_F5),
                            6 => Some(Key::KEY_F6),
                            7 => Some(Key::KEY_F7),
                            8 => Some(Key::KEY_F8),
                            9 => Some(Key::KEY_F9),
                            10 => Some(Key::KEY_F10),
                            11 => Some(Key::KEY_F11),
                            12 => Some(Key::KEY_F12),
                            13 => Some(Key::KEY_F13),
                            14 => Some(Key::KEY_F14),
                            15 => Some(Key::KEY_F15),
                            16 => Some(Key::KEY_F16),
                            17 => Some(Key::KEY_F17),
                            18 => Some(Key::KEY_F18),
                            19 => Some(Key::KEY_F19),
                            20 => Some(Key::KEY_F20),
                            21 => Some(Key::KEY_F21),
                            22 => Some(Key::KEY_F22),
                            23 => Some(Key::KEY_F23),
                            24 => Some(Key::KEY_F24),
                            _ => None,
                        };
                        if let Some(key) = key {
                            return Ok(key);
                        }
                    }
                }

                // Letters
                if upper.len() == 1 {
                    return match upper.as_str() {
                        "A" => Ok(Key::KEY_A),
                        "B" => Ok(Key::KEY_B),
                        "C" => Ok(Key::KEY_C),
                        "D" => Ok(Key::KEY_D),
                        "E" => Ok(Key::KEY_E),
                        "F" => Ok(Key::KEY_F),
                        "G" => Ok(Key::KEY_G),
                        "H" => Ok(Key::KEY_H),
                        "I" => Ok(Key::KEY_I),
                        "J" => Ok(Key::KEY_J),
                        "K" => Ok(Key::KEY_K),
                        "L" => Ok(Key::KEY_L),
                        "M" => Ok(Key::KEY_M),
                        "N" => Ok(Key::KEY_N),
                        "O" => Ok(Key::KEY_O),
                        "P" => Ok(Key::KEY_P),
                        "Q" => Ok(Key::KEY_Q),
                        "R" => Ok(Key::KEY_R),
                        "S" => Ok(Key::KEY_S),
                        "T" => Ok(Key::KEY_T),
                        "U" => Ok(Key::KEY_U),
                        "V" => Ok(Key::KEY_V),
                        "W" => Ok(Key::KEY_W),
                        "X" => Ok(Key::KEY_X),
                        "Y" => Ok(Key::KEY_Y),
                        "Z" => Ok(Key::KEY_Z),
                        "0" => Ok(Key::KEY_0),
                        "1" => Ok(Key::KEY_1),
                        "2" => Ok(Key::KEY_2),
                        "3" => Ok(Key::KEY_3),
                        "4" => Ok(Key::KEY_4),
                        "5" => Ok(Key::KEY_5),
                        "6" => Ok(Key::KEY_6),
                        "7" => Ok(Key::KEY_7),
                        "8" => Ok(Key::KEY_8),
                        "9" => Ok(Key::KEY_9),
                        _ => Err(anyhow::anyhow!("Unsupported hotkey key: {trimmed}")),
                    };
                }

                anyhow::bail!("Unsupported hotkey key: {trimmed}");
            }
        };

        Ok(mapped)
    }

    fn run_loop(app: AppHandle, spec: HotkeySpec, stop_rx: Receiver<()>) -> anyhow::Result<()> {
        let mut manager = DeviceManager::new()?;
        info!(
            "evdev hotkeys active: key={:?} ctrl={} alt={} shift={} meta={} devices={}",
            spec.key,
            spec.modifiers.ctrl,
            spec.modifiers.alt,
            spec.modifiers.shift,
            spec.modifiers.meta,
            manager.devices.len()
        );

        let mut held_ctrl: HashSet<Key> = HashSet::new();
        let mut held_alt: HashSet<Key> = HashSet::new();
        let mut held_shift: HashSet<Key> = HashSet::new();
        let mut held_meta: HashSet<Key> = HashSet::new();
        let mut is_pressed = false;
        let mut last_validation = Instant::now();
        let mut warned_no_devices = false;

        loop {
            match stop_rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => {
                    debug!("evdev hotkeys stopping");
                    return Ok(());
                }
                Err(TryRecvError::Empty) => {}
            }

            if manager.check_for_device_changes() {
                held_ctrl.clear();
                held_alt.clear();
                held_shift.clear();
                held_meta.clear();
                is_pressed = false;
                manager.handle_device_changes();
            }

            if last_validation.elapsed() > Duration::from_secs(30) {
                manager.validate_devices();
                last_validation = Instant::now();
            }

            if manager.devices.is_empty() {
                if !warned_no_devices {
                    warned_no_devices = true;
                    warn!("No readable keyboard devices available. Hotkeys will not work until permissions are granted.");
                }
                thread::sleep(Duration::from_secs(1));
                manager.enumerate_devices();
                continue;
            }

            warned_no_devices = false;

            for (key, value) in manager.poll_events() {
                update_modifier_state(key, value, &mut held_ctrl, &mut held_alt, &mut held_shift, &mut held_meta);

                if key != spec.key {
                    continue;
                }

                if !modifiers_satisfied(spec.modifiers, &held_ctrl, &held_alt, &held_shift, &held_meta) {
                    continue;
                }

                match value {
                    1 if !is_pressed => {
                        is_pressed = true;
                        handle_hotkey_state(&app, HotkeyState::Pressed);
                    }
                    0 if is_pressed => {
                        is_pressed = false;
                        handle_hotkey_state(&app, HotkeyState::Released);
                    }
                    2 => {
                        // repeat - ignore
                    }
                    _ => {}
                }
            }

            thread::sleep(Duration::from_millis(5));
        }
    }

    fn modifiers_satisfied(
        required: Modifiers,
        held_ctrl: &HashSet<Key>,
        held_alt: &HashSet<Key>,
        held_shift: &HashSet<Key>,
        held_meta: &HashSet<Key>,
    ) -> bool {
        if required.ctrl && held_ctrl.is_empty() {
            return false;
        }
        if required.alt && held_alt.is_empty() {
            return false;
        }
        if required.shift && held_shift.is_empty() {
            return false;
        }
        if required.meta && held_meta.is_empty() {
            return false;
        }
        true
    }

    fn update_modifier_state(
        key: Key,
        value: i32,
        held_ctrl: &mut HashSet<Key>,
        held_alt: &mut HashSet<Key>,
        held_shift: &mut HashSet<Key>,
        held_meta: &mut HashSet<Key>,
    ) {
        let is_down = value == 1;
        let is_up = value == 0;

        match key {
            Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => {
                if is_down {
                    held_ctrl.insert(key);
                } else if is_up {
                    held_ctrl.remove(&key);
                }
            }
            Key::KEY_LEFTALT | Key::KEY_RIGHTALT => {
                if is_down {
                    held_alt.insert(key);
                } else if is_up {
                    held_alt.remove(&key);
                }
            }
            Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => {
                if is_down {
                    held_shift.insert(key);
                } else if is_up {
                    held_shift.remove(&key);
                }
            }
            Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => {
                if is_down {
                    held_meta.insert(key);
                } else if is_up {
                    held_meta.remove(&key);
                }
            }
            _ => {}
        }
    }

    struct DeviceManager {
        devices: HashMap<PathBuf, Device>,
        inotify: Inotify,
        inotify_buffer: [u8; 1024],
    }

    impl DeviceManager {
        fn new() -> anyhow::Result<Self> {
            let inotify = Inotify::init().map_err(|err| anyhow::anyhow!(err))?;
            inotify
                .watches()
                .add("/dev/input", WatchMask::CREATE | WatchMask::DELETE)
                .map_err(|err| anyhow::anyhow!(err))?;

            // Ensure inotify reads are non-blocking so the hotkey loop can poll.
            set_fd_nonblocking(inotify.as_raw_fd());

            let mut manager = Self {
                devices: HashMap::new(),
                inotify,
                inotify_buffer: [0u8; 1024],
            };
            manager.enumerate_devices();
            Ok(manager)
        }

        fn enumerate_devices(&mut self) {
            let Ok(dir) = std::fs::read_dir("/dev/input") else {
                return;
            };

            for entry in dir.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !name.starts_with("event") {
                    continue;
                }
                if self.devices.contains_key(&path) {
                    continue;
                }

                match Device::open(&path) {
                    Ok(device) => {
                        if is_keyboard(&device) {
                            let device_name = device.name().unwrap_or("unknown");
                            if device_name == VIRTUAL_KEYBOARD_NAME {
                                continue;
                            }

                            set_nonblocking(&device);
                            self.devices.insert(path.clone(), device);
                        }
                    }
                    Err(_err) => {
                        // ignore (permission denied, not present, etc.)
                    }
                }
            }
        }

        fn check_for_device_changes(&mut self) -> bool {
            let events = match self.inotify.read_events(&mut self.inotify_buffer) {
                Ok(events) => events,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    return false;
                }
                Err(_) => {
                    return false;
                }
            };

            let mut changed = false;
            for event in events {
                if let Some(name) = event.name {
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("event") {
                        changed = true;
                        if event.mask.contains(inotify::EventMask::DELETE) {
                            let path = PathBuf::from("/dev/input").join(&*name_str);
                            self.devices.remove(&path);
                        }
                    }
                }
            }
            changed
        }

        fn handle_device_changes(&mut self) {
            thread::sleep(Duration::from_millis(150));
            self.enumerate_devices();
            debug!("Devices updated: {} device(s) active", self.devices.len());
        }

        fn validate_devices(&mut self) {
            let mut stale = Vec::new();
            for (path, device) in &self.devices {
                let fd = device.as_raw_fd();
                let link_path = format!("/proc/self/fd/{}", fd);
                let valid = std::fs::read_link(&link_path)
                    .map(|target| target.exists())
                    .unwrap_or(false);
                if !valid {
                    stale.push(path.clone());
                }
            }
            for path in stale {
                self.devices.remove(&path);
            }
        }

        fn poll_events(&mut self) -> Vec<(Key, i32)> {
            let mut out = Vec::new();
            let mut remove = Vec::new();

            for (path, device) in &mut self.devices {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            if let InputEventKind::Key(key) = event.kind() {
                                out.push((key, event.value()));
                            }
                        }
                    }
                    Err(err) if err.raw_os_error() == Some(libc::ENODEV) => {
                        remove.push(path.clone());
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        // normal
                    }
                    Err(_err) => {
                        // ignore
                    }
                }
            }

            for path in remove {
                self.devices.remove(&path);
            }

            out
        }
    }

    fn is_keyboard(device: &Device) -> bool {
        device
            .supported_keys()
            .map(|keys| {
                keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) && keys.contains(Key::KEY_ENTER)
            })
            .unwrap_or(false)
    }

    fn set_nonblocking(device: &Device) {
        let fd = device.as_raw_fd();
        set_fd_nonblocking(fd);
    }

    fn set_fd_nonblocking(fd: i32) {
        unsafe {
            let flags = fcntl(fd, F_GETFL);
            if flags != -1 {
                let _ = fcntl(fd, F_SETFL, flags | O_NONBLOCK);
            }
        }
    }
}

fn register_evdev_shortcut(app: &AppHandle, shortcut: &str) -> tauri::Result<()> {
    match linux_evdev::start(app, shortcut) {
        Ok(()) => Ok(()),
        Err(error) => {
            warn!("evdev hotkey registration failed: {error}");
            let _ = app.emit(
                "hotkey-error",
                format!(
                    "Failed to enable global hotkeys. On Linux Wayland this requires access to /dev/input (input group) and a logout/login. Error: {error}"
                ),
            );
            Err(tauri::Error::from(anyhow::anyhow!(error.to_string())))
        }
    }
}

fn stop_evdev_listener() {
    linux_evdev::stop_from_parent();
}
