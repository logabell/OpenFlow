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

fn is_wayland_session() -> bool {
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    xdg_session_type == "wayland" || !wayland_display.is_empty()
}

fn has_x11_display() -> bool {
    std::env::var("DISPLAY")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

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
        "Registering hotkey: {} (session_type={}, display={})",
        shortcut,
        session_type,
        std::env::var("DISPLAY").unwrap_or_default()
    );

    // Preferred backend selection:
    // - Wayland: evdev (global hotkeys via /dev/input)
    // - X11: X11 grabs (no /dev/input needed; works in VNC/Xvfb)
    if !is_wayland_session() && has_x11_display() {
        match register_x11_shortcut(app, shortcut) {
            Ok(()) => {
                set_current_hotkey(shortcut);
                let _ = app.emit("hotkey-backend", "x11");
            }
            Err(error) => {
                warn!("x11 hotkey registration failed: {error}");
                register_evdev_shortcut(app, shortcut)?;
                set_current_hotkey(shortcut);
                let _ = app.emit("hotkey-backend", "evdev");
            }
        }
    } else {
        register_evdev_shortcut(app, shortcut)?;
        set_current_hotkey(shortcut);
        let _ = app.emit("hotkey-backend", "evdev");
    }
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
        stop_x11_listener();
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

// -------------------------------------------------------------------------------------------------
// Linux X11 backend (core grabs)
// -------------------------------------------------------------------------------------------------

mod linux_x11 {
    use super::{handle_hotkey_state, HotkeyState};
    use anyhow::Context;
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::thread;
    use std::time::Duration;
    use tauri::AppHandle;
    use tracing::info;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask};
    use x11rb::protocol::Event;

    // Minimal X11 keysym constants we need.
    // Values from X11/keysymdef.h.
    const XK_SPACE: u32 = 0x0020;
    const XK_TAB: u32 = 0xff09;
    const XK_RETURN: u32 = 0xff0d;
    const XK_ESCAPE: u32 = 0xff1b;

    const XK_SHIFT_L: u32 = 0xffe1;
    const XK_SHIFT_R: u32 = 0xffe2;
    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_CONTROL_R: u32 = 0xffe4;
    const XK_META_L: u32 = 0xffe7;
    const XK_META_R: u32 = 0xffe8;
    const XK_ALT_L: u32 = 0xffe9;
    const XK_ALT_R: u32 = 0xffea;
    const XK_SUPER_L: u32 = 0xffeb;
    const XK_SUPER_R: u32 = 0xffec;

    const XK_MODE_SWITCH: u32 = 0xff7e;
    const XK_NUM_LOCK: u32 = 0xff7f;
    const XK_ISO_LEVEL3_SHIFT: u32 = 0xfe03;

    const XK_F1: u32 = 0xffbe;

    pub(super) struct X11Listener {
        stop_tx: Sender<()>,
        thread: thread::JoinHandle<()>,
    }

    static X11_LISTENER: parking_lot::RwLock<Option<X11Listener>> =
        parking_lot::RwLock::new(None);

    #[derive(Debug, Clone, Copy)]
    struct Modifiers {
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    }

    #[derive(Debug, Clone, Copy)]
    struct HotkeySpec {
        keycode: u8,
        required: u16,
    }

    pub(super) fn start(app: &AppHandle, shortcut: &str) -> anyhow::Result<()> {
        stop();

        let (mods, key_str) = parse_hotkey(shortcut)?;

        let (conn, screen_num) = x11rb::connect(None).context("connect to X11")?;
        let root = conn.setup().roots[screen_num].root;

        // Resolve trigger keycode.
        let keycode = keycode_for_key_string(&conn, key_str)?;

        // Compute modifier masks from the server's modifier map so Alt/Meta work across layouts.
        let modifier_map = ModifierMap::new(&conn)?;
        let mut required_mask: u16 = 0;
        if mods.shift {
            required_mask |= u16::from(ModMask::SHIFT);
        }
        if mods.ctrl {
            required_mask |= u16::from(ModMask::CONTROL);
        }
        if mods.alt {
            required_mask |= u16::from(modifier_map.alt);
        }
        if mods.meta {
            required_mask |= u16::from(modifier_map.meta);
        }

        // Grab the key. Include lock variants so the grab still works with CapsLock/NumLock.
        let variants = modifier_map.lock_variants();
        for extra in variants {
            let mask_bits = required_mask | extra;
            let mask = ModMask::from(mask_bits);
            let _ = conn.grab_key(
                false,
                root,
                mask,
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?;
        }

        conn.flush()?;

        info!(
            "x11 hotkeys active: keycode={} required_mask=0x{:x}",
            keycode, required_mask
        );

        let app_handle = app.clone();
        let (stop_tx, stop_rx) = channel();
        let thread = thread::Builder::new()
            .name("x11-hotkeys".to_string())
            .spawn(move || {
                if let Err(error) = run_loop(conn, app_handle, HotkeySpec { keycode, required: required_mask }, stop_rx) {
                    tracing::warn!("x11 hotkey listener stopped: {error:?}");
                }
            })?;

        *X11_LISTENER.write() = Some(X11Listener { stop_tx, thread });
        Ok(())
    }

    pub(super) fn stop() {
        let listener = X11_LISTENER.write().take();
        if let Some(listener) = listener {
            let _ = listener.stop_tx.send(());
            let _ = listener.thread.join();
        }
    }

    pub(super) fn stop_from_parent() {
        stop();
    }

    fn parse_hotkey(input: &str) -> anyhow::Result<(Modifiers, &str)> {
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

        Ok((modifiers, key_str))
    }

    struct ModifierMap {
        alt: ModMask,
        meta: ModMask,
        num: ModMask,
        lock: ModMask,
    }

    impl ModifierMap {
        fn new<C: Connection>(conn: &C) -> anyhow::Result<Self> {
            let reply = conn
                .get_modifier_mapping()
                .context("get_modifier_mapping")?
                .reply()
                .context("read modifier mapping")?;

            let keycodes_per_mod = reply.keycodes_per_modifier() as usize;
            let mods = reply.keycodes;

            let alt_code =
                keycode_for_any_keysym(conn, &[XK_ALT_L, XK_ALT_R, XK_ISO_LEVEL3_SHIFT, XK_MODE_SWITCH]).ok();

            let meta_code = keycode_for_any_keysym(conn, &[XK_SUPER_L, XK_SUPER_R, XK_META_L, XK_META_R]).ok();

            let num_code = keycode_for_any_keysym(conn, &[XK_NUM_LOCK]).ok();

            let mut alt = ModMask::M1;
            let mut meta = ModMask::M4;
            let mut num = ModMask::from(0u16);

            for mod_index in 0..8 {
                let start = mod_index * keycodes_per_mod;
                let end = start + keycodes_per_mod;
                let slice = &mods[start..end];
                if alt_code.is_some() && slice.iter().any(|&c| c != 0 && Some(c) == alt_code) {
                    alt = mask_for_index(mod_index);
                }
                if meta_code.is_some() && slice.iter().any(|&c| c != 0 && Some(c) == meta_code) {
                    meta = mask_for_index(mod_index);
                }
                if num_code.is_some() && slice.iter().any(|&c| c != 0 && Some(c) == num_code) {
                    num = mask_for_index(mod_index);
                }
            }

            Ok(Self {
                alt,
                meta,
                num,
                lock: ModMask::LOCK,
            })
        }

        fn lock_variants(&self) -> Vec<u16> {
            let mut out = vec![0u16];
            let lock: u16 = self.lock.into();
            let num: u16 = self.num.into();
            if lock != 0 {
                out.push(lock);
            }
            if num != 0 {
                out.push(num);
            }
            if lock != 0 && num != 0 {
                out.push(lock | num);
            }
            out
        }
    }

    fn mask_for_index(i: usize) -> ModMask {
        match i {
            0 => ModMask::SHIFT,
            1 => ModMask::LOCK,
            2 => ModMask::CONTROL,
            3 => ModMask::M1,
            4 => ModMask::M2,
            5 => ModMask::M3,
            6 => ModMask::M4,
            7 => ModMask::M5,
            _ => ModMask::from(0u16),
        }
    }

    fn keycode_for_key_string<C: Connection>(conn: &C, key: &str) -> anyhow::Result<u8> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            anyhow::bail!("missing hotkey key");
        }

        let upper = trimmed.to_ascii_uppercase().replace(' ', "");
        let candidates: Vec<u32> = match upper.as_str() {
            "SPACE" => vec![XK_SPACE],
            "ENTER" | "RETURN" => vec![XK_RETURN],
            "ESC" | "ESCAPE" => vec![XK_ESCAPE],
            "TAB" => vec![XK_TAB],

            "RIGHTALT" | "ALTRIGHT" => vec![XK_ALT_R, XK_ISO_LEVEL3_SHIFT, XK_MODE_SWITCH],
            "LEFTALT" | "ALTLEFT" => vec![XK_ALT_L],
            "RIGHTCTRL" | "CTRLRIGHT" | "CONTROLRIGHT" => vec![XK_CONTROL_R],
            "LEFTCTRL" | "CTRLLEFT" | "CONTROLLEFT" => vec![XK_CONTROL_L],
            "RIGHTSHIFT" | "SHIFTRIGHT" => vec![XK_SHIFT_R],
            "LEFTSHIFT" | "SHIFTLEFT" => vec![XK_SHIFT_L],
            "RIGHTMETA" | "METARIGHT" | "SUPERRIGHT" => vec![XK_SUPER_R, XK_META_R],
            "LEFTMETA" | "METALEFT" | "SUPERLEFT" => vec![XK_SUPER_L, XK_META_L],
            _ => {
                // Function keys
                if let Some(num) = upper.strip_prefix('F') {
                    if let Ok(n) = num.parse::<u8>() {
                        let base = XK_F1;
                        if (1..=24).contains(&n) {
                            return Ok(
                                keycode_for_any_keysym(conn, &[base + (n as u32) - 1])
                                    .context("resolve function key")?,
                            );
                        }
                    }
                }

                // Single ASCII letter/digit
                if upper.len() == 1 {
                    let ch = upper.as_bytes()[0];
                    return match ch {
                        b'A'..=b'Z' => {
                            let ks = (ch as u32) as u32;
                            keycode_for_any_keysym(conn, &[ks])
                        }
                        b'0'..=b'9' => {
                            let ks = (ch as u32) as u32;
                            keycode_for_any_keysym(conn, &[ks])
                        }
                        _ => anyhow::bail!("Unsupported hotkey key: {trimmed}"),
                    };
                }

                anyhow::bail!("Unsupported hotkey key: {trimmed}");
            }
        };

        keycode_for_any_keysym(conn, &candidates)
    }

    fn keycode_for_any_keysym<C: Connection>(conn: &C, keysyms: &[u32]) -> anyhow::Result<u8> {
        for &keysym in keysyms {
            if let Some(code) = keycode_for_keysym(conn, keysym)? {
                return Ok(code);
            }
        }
        anyhow::bail!("no matching keycode found")
    }

    fn keycode_for_keysym<C: Connection>(conn: &C, keysym: u32) -> anyhow::Result<Option<u8>> {
        let setup = conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        if max < min {
            return Ok(None);
        }

        let count = u8::from(max) - u8::from(min) + 1;
        let reply = conn
            .get_keyboard_mapping(min, count)
            .context("get_keyboard_mapping")?
            .reply()
            .context("read keyboard mapping")?;

        let per = reply.keysyms_per_keycode as usize;
        if per == 0 {
            return Ok(None);
        }

        for (i, chunk) in reply.keysyms.chunks(per).enumerate() {
            let keycode = u8::from(min) + i as u8;
            if chunk.iter().any(|&k| k == keysym) {
                return Ok(Some(keycode));
            }
        }

        Ok(None)
    }

    fn run_loop<C: Connection>(
        conn: C,
        app: AppHandle,
        spec: HotkeySpec,
        stop_rx: Receiver<()>,
    ) -> anyhow::Result<()> {
        let mut is_pressed = false;
        loop {
            match stop_rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => {}
            }

            if let Some(event) = conn.poll_for_event()? {
                match event {
                    Event::KeyPress(ev) => {
                        if ev.detail == spec.keycode {
                            let state_bits: u16 = ev.state.into();
                            if (state_bits & spec.required) == spec.required {
                                if !is_pressed {
                                    is_pressed = true;
                                    handle_hotkey_state(&app, HotkeyState::Pressed);
                                }
                            }
                        }
                    }
                    Event::KeyRelease(ev) => {
                        if ev.detail == spec.keycode {
                            if is_pressed {
                                is_pressed = false;
                                handle_hotkey_state(&app, HotkeyState::Released);
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                thread::sleep(Duration::from_millis(8));
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

fn register_x11_shortcut(app: &AppHandle, shortcut: &str) -> tauri::Result<()> {
    match linux_x11::start(app, shortcut) {
        Ok(()) => Ok(()),
        Err(error) => {
            warn!("x11 hotkey registration failed: {error}");
            let _ = app.emit(
                "hotkey-error",
                format!(
                    "Failed to enable global hotkeys on X11. Ensure DISPLAY is set and XInput/XTEST are available on the X server. Error: {error}"
                ),
            );
            Err(tauri::Error::from(anyhow::anyhow!(error.to_string())))
        }
    }
}

fn stop_evdev_listener() {
    linux_evdev::stop_from_parent();
}

fn stop_x11_listener() {
    linux_x11::stop_from_parent();
}
