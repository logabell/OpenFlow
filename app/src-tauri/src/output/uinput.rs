#[cfg(target_os = "linux")]
use evdev::{uinput::VirtualDeviceBuilder, AttributeSet, EventType, InputEvent, Key};

#[cfg(target_os = "linux")]
use once_cell::sync::Lazy;

#[cfg(target_os = "linux")]
use parking_lot::Mutex;

#[cfg(target_os = "linux")]
use std::thread::sleep;

#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use super::PasteShortcut;

#[cfg(target_os = "linux")]
// This string can show up in tools that list input devices.
pub const VIRTUAL_KEYBOARD_NAME: &str = "OpenFlow Virtual Keyboard";

#[cfg(target_os = "linux")]
static VIRTUAL_KEYBOARD: Lazy<Mutex<Option<evdev::uinput::VirtualDevice>>> =
    Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "linux")]
fn get_or_create_virtual_keyboard() -> anyhow::Result<()> {
    let mut guard = VIRTUAL_KEYBOARD.lock();
    if guard.is_some() {
        return Ok(());
    }

    let mut keys = AttributeSet::<Key>::new();
    keys.insert(Key::KEY_LEFTCTRL);
    keys.insert(Key::KEY_LEFTSHIFT);
    keys.insert(Key::KEY_V);

    let device = VirtualDeviceBuilder::new()
        .map_err(|err| anyhow::anyhow!(err))?
        .name(VIRTUAL_KEYBOARD_NAME)
        .with_keys(&keys)
        .map_err(|err| anyhow::anyhow!(err))?
        .build()
        .map_err(|err| anyhow::anyhow!(err))?;

    *guard = Some(device);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn send_paste(shortcut: PasteShortcut) -> anyhow::Result<()> {
    get_or_create_virtual_keyboard()?;

    let mut guard = VIRTUAL_KEYBOARD.lock();
    let device = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("virtual keyboard not initialized"))?;

    let event_type = EventType::KEY;
    let ctrl = Key::KEY_LEFTCTRL.code();
    let shift = Key::KEY_LEFTSHIFT.code();
    let v = Key::KEY_V.code();

    let mut down_events = Vec::with_capacity(3);
    down_events.push(InputEvent::new(event_type, ctrl, 1));
    if matches!(shortcut, PasteShortcut::CtrlShiftV) {
        down_events.push(InputEvent::new(event_type, shift, 1));
    }
    down_events.push(InputEvent::new(event_type, v, 1));
    device
        .emit(&down_events)
        .map_err(|err| anyhow::anyhow!(err))?;

    // A tiny delay helps some apps detect the chord reliably.
    sleep(Duration::from_millis(15));

    let mut up_events = Vec::with_capacity(3);
    up_events.push(InputEvent::new(event_type, v, 0));
    if matches!(shortcut, PasteShortcut::CtrlShiftV) {
        up_events.push(InputEvent::new(event_type, shift, 0));
    }
    up_events.push(InputEvent::new(event_type, ctrl, 0));
    device
        .emit(&up_events)
        .map_err(|err| anyhow::anyhow!(err))?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn send_paste(_shortcut: super::PasteShortcut) -> anyhow::Result<()> {
    anyhow::bail!("uinput paste injection is only supported on Linux")
}

#[cfg(not(target_os = "linux"))]
pub const VIRTUAL_KEYBOARD_NAME: &str = "";
