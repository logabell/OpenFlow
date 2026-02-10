use crate::output::PasteShortcut;

use anyhow::Context;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;

// Minimal X11 keysym constants we need.
// Values from X11/keysymdef.h.
const XK_CONTROL_L: u32 = 0xffe3;
const XK_CONTROL_R: u32 = 0xffe4;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_SHIFT_R: u32 = 0xffe2;
const XK_V_UPPER: u32 = 0x0056;
const XK_V_LOWER: u32 = 0x0076;

fn is_wayland_session() -> bool {
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    xdg_session_type == "wayland" || !wayland_display.is_empty()
}

pub fn send_paste(shortcut: PasteShortcut) -> anyhow::Result<()> {
    // This backend is only intended for X11.
    if is_wayland_session() {
        anyhow::bail!("x11 paste backend is not available on Wayland");
    }

    let display = std::env::var("DISPLAY").unwrap_or_default();
    if display.trim().is_empty() {
        anyhow::bail!("DISPLAY is not set");
    }

    let (conn, screen_num) = x11rb::connect(None).context("connect to X11")?;
    let root = conn.setup().roots[screen_num].root;

    // Ensure XTEST is present.
    let xtest = conn
        .query_extension(b"XTEST")
        .context("query XTEST extension")?
        .reply()
        .context("read XTEST extension reply")?;
    if !xtest.present {
        anyhow::bail!("XTEST extension not available");
    }

    let ctrl = keycode_for_any_keysym(&conn, &[XK_CONTROL_L, XK_CONTROL_R])
        .context("resolve Control keycode")?;
    let shift = keycode_for_any_keysym(&conn, &[XK_SHIFT_L, XK_SHIFT_R])
        .context("resolve Shift keycode")?;

    // Prefer lowercase v. Keycode is layout-dependent.
    let v =
        keycode_for_any_keysym(&conn, &[XK_V_LOWER, XK_V_UPPER]).context("resolve V keycode")?;

    use x11rb::protocol::xproto;
    use x11rb::protocol::xtest::ConnectionExt as _;

    let press = xproto::KEY_PRESS_EVENT;
    let release = xproto::KEY_RELEASE_EVENT;

    // Press
    conn.xtest_fake_input(press, ctrl, 0, root, 0, 0, 0)
        .context("xtest ctrl down")?;
    if matches!(shortcut, PasteShortcut::CtrlShiftV) {
        conn.xtest_fake_input(press, shift, 0, root, 0, 0, 0)
            .context("xtest shift down")?;
    }
    conn.xtest_fake_input(press, v, 0, root, 0, 0, 0)
        .context("xtest v down")?;

    // Release
    conn.xtest_fake_input(release, v, 0, root, 0, 0, 0)
        .context("xtest v up")?;
    if matches!(shortcut, PasteShortcut::CtrlShiftV) {
        conn.xtest_fake_input(release, shift, 0, root, 0, 0, 0)
            .context("xtest shift up")?;
    }
    conn.xtest_fake_input(release, ctrl, 0, root, 0, 0, 0)
        .context("xtest ctrl up")?;

    conn.flush().context("flush X11")?;
    Ok(())
}

fn keycode_for_any_keysym<C: x11rb::connection::Connection>(
    conn: &C,
    keysyms: &[u32],
) -> anyhow::Result<u8> {
    for &keysym in keysyms {
        if let Some(code) = keycode_for_keysym(conn, keysym)? {
            return Ok(code);
        }
    }
    anyhow::bail!("no matching keycode found")
}

fn keycode_for_keysym<C: x11rb::connection::Connection>(
    conn: &C,
    keysym: u32,
) -> anyhow::Result<Option<u8>> {
    use x11rb::protocol::xproto::ConnectionExt as _;

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
