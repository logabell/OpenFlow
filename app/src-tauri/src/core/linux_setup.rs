use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxPermissionsStatus {
    pub supported: bool,
    pub wayland_session: bool,
    pub x11_session: bool,
    pub x11_display_available: bool,
    pub x11_hotkeys_available: bool,
    pub x11_xtest_available: bool,
    pub xdg_runtime_dir_available: bool,
    pub evdev_readable: bool,
    pub uinput_writable: bool,
    pub clipboard_backend: String,
    pub wl_copy_available: bool,
    pub wl_paste_available: bool,
    pub xclip_available: bool,
    pub pkexec_available: bool,
    pub setfacl_available: bool,
    pub details: Vec<String>,
}

pub fn permissions_status() -> LinuxPermissionsStatus {
    let mut details = Vec::new();

    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    let wayland_session = xdg_session_type == "wayland" || !wayland_display.is_empty();
    if !wayland_session {
        let session = if xdg_session_type.is_empty() {
            "unset".to_string()
        } else {
            xdg_session_type.clone()
        };
        details.push(format!(
            "Not running under Wayland (XDG_SESSION_TYPE={session})"
        ));
    }

    let xdg_runtime_dir_available = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|value| std::path::Path::new(&value).is_dir())
        .unwrap_or(false);

    let display = std::env::var("DISPLAY").unwrap_or_default();
    let x11_session = !wayland_session && !display.trim().is_empty();

    let (x11_display_available, x11_hotkeys_available, x11_xtest_available) = if x11_session {
        match check_x11_capabilities() {
            Ok((display_ok, xtest_ok)) => {
                if display_ok && !xtest_ok {
                    details.push("Missing XTEST (X11 paste injection may not work)".to_string());
                }
                // Hotkeys use core X11 grabs (no /dev/input needed). If we can connect, we can at
                // least attempt grabs.
                (display_ok, display_ok, xtest_ok)
            }
            Err(message) => {
                details.push(message);
                (false, false, false)
            }
        }
    } else {
        (false, false, false)
    };

    let (evdev_readable, uinput_writable) = if wayland_session {
        let evdev_readable = match check_evdev_keyboard_access() {
            Ok(()) => true,
            Err(message) => {
                details.push(message);
                false
            }
        };

        let uinput_writable = match check_uinput_access() {
            Ok(()) => true,
            Err(message) => {
                details.push(message);
                if let Some(hint) = diagnose_uinput_acl_hint() {
                    details.push(hint);
                }
                false
            }
        };

        (evdev_readable, uinput_writable)
    } else {
        (false, false)
    };

    let wl_copy_available = binary_in_path("wl-copy");

    let wl_paste_available = binary_in_path("wl-paste");

    let xclip_available = binary_in_path("xclip");

    let clipboard_backend = if wayland_session { "wayland" } else { "x11" };

    if wayland_session {
        if !xdg_runtime_dir_available {
            details.push("Missing XDG_RUNTIME_DIR (Wayland clipboard may not work)".to_string());
        }
        if !wl_copy_available {
            details.push("Missing wl-copy (install wl-clipboard)".to_string());
        }
        if !wl_paste_available {
            details.push("Missing wl-paste (install wl-clipboard)".to_string());
        }
    } else if !xclip_available {
        details.push("Missing xclip (install xclip for X11 clipboard)".to_string());
    }

    let pkexec_available = binary_in_path("pkexec");
    if wayland_session && !pkexec_available {
        details.push("Missing pkexec (install polkit)".to_string());
    }

    let setfacl_available = binary_in_path("setfacl");
    if wayland_session && !setfacl_available {
        details.push("Missing setfacl (install acl)".to_string());
    }

    LinuxPermissionsStatus {
        supported: true,
        wayland_session,
        x11_session,
        x11_display_available,
        x11_hotkeys_available,
        x11_xtest_available,
        xdg_runtime_dir_available,
        evdev_readable,
        uinput_writable,
        clipboard_backend: clipboard_backend.to_string(),
        wl_copy_available,
        wl_paste_available,
        xclip_available,
        pkexec_available,
        setfacl_available,
        details,
    }
}

fn check_x11_capabilities() -> Result<(bool, bool), String> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    let display = std::env::var("DISPLAY").unwrap_or_default();
    if display.trim().is_empty() {
        return Ok((false, false));
    }

    let (conn, _) =
        x11rb::connect(None).map_err(|err| format!("Failed to connect to X11: {err}"))?;

    let xtest = conn
        .query_extension(b"XTEST")
        .map_err(|err| format!("Failed to query XTEST extension: {err}"))?
        .reply()
        .map_err(|err| format!("Failed to read XTEST extension reply: {err}"))?;

    Ok((true, xtest.present))
}

fn diagnose_uinput_acl_hint() -> Option<String> {
    use std::process::Command;

    // Only useful when /dev/uinput exists but isn't writable.
    if !std::path::Path::new("/dev/uinput").exists() {
        return None;
    }

    let getfacl = if std::path::Path::new("/usr/bin/getfacl").is_file() {
        "/usr/bin/getfacl"
    } else if binary_in_path("getfacl") {
        "getfacl"
    } else {
        return None;
    };

    let output = Command::new(getfacl)
        .args(["-p", "/dev/uinput"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("group::---") {
        return Some(
            "uinput ACL blocks the 'input' group (group::---). This is commonly caused by brltty; the one-click setup uses setfacl to fix it."
                .to_string(),
        );
    }

    None
}

pub fn enable_permissions_for_current_user() -> anyhow::Result<()> {
    let user = current_username().unwrap_or_default();
    if user.is_empty() {
        anyhow::bail!("Could not determine current user (unable to resolve username)");
    }

    // Restrict to typical Unix usernames to avoid passing unsafe values to a root shell.
    if !user
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        anyhow::bail!("Invalid username '{user}'");
    }

    if !binary_in_path("pkexec") {
        anyhow::bail!("pkexec not found (install polkit)");
    }

    // Keep heredoc terminators at column 0 (no indentation) so shells parse them correctly.
    let script = r#"set -eu

USER_NAME="$1"

if ! command -v getent >/dev/null 2>&1; then
  echo "getent not available" >&2
  exit 1
fi

if ! getent group input >/dev/null 2>&1; then
  groupadd input
fi

if [ ! -x /usr/bin/setfacl ]; then
  echo "setfacl not available (install acl)" >&2
  exit 1
fi

# Add user to input group (for /dev/input and /dev/uinput access).
usermod -a -G input "$USER_NAME"

# Ensure uinput is available.
if command -v modprobe >/dev/null 2>&1; then
  modprobe uinput || true
fi

# Make /dev/uinput writable by the input group.
RULE_FILE="/etc/udev/rules.d/99-openflow-uinput.rules"
cat > "$RULE_FILE" <<'EOF'
KERNEL=="uinput", ACTION=="add", MODE="0660", GROUP="input", TEST=="/usr/bin/setfacl", RUN+="/usr/bin/setfacl -m g::rw -m m::rw /dev/$name"
EOF

# Apply immediately for the current node (if present).
if [ -e /dev/uinput ]; then
  chgrp input /dev/uinput || true
  chmod 0660 /dev/uinput || true
  /usr/bin/setfacl -m g::rw -m m::rw /dev/uinput || true
fi

if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules || true
  udevadm trigger --action=add --name-match=uinput || true
fi
"#;

    let pkexec = if std::path::Path::new("/usr/bin/pkexec").is_file() {
        "/usr/bin/pkexec"
    } else {
        "pkexec"
    };

    let status = std::process::Command::new(pkexec)
        .arg("sh")
        .arg("-c")
        .arg(script)
        .arg("_")
        .arg(&user)
        .status()?;

    if !status.success() {
        anyhow::bail!("pkexec failed with status {status}");
    }

    Ok(())
}

fn current_username() -> Option<String> {
    // Avoid relying on $USER, which may be missing in clean/sandboxed environments.
    if let Ok(u) = std::env::var("USER") {
        if !u.trim().is_empty() {
            return Some(u);
        }
    }

    username_from_uid(unsafe { libc::getuid() })
}

fn username_from_uid(uid: libc::uid_t) -> Option<String> {
    // getpwuid_r is the most reliable way to map uid -> username.
    // This follows the POSIX pattern with a dynamically sized buffer.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();

        let mut buf_len = libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX);
        if buf_len < 0 {
            buf_len = 16 * 1024;
        }

        // Cap the buffer to a reasonable size to avoid huge allocations.
        let buf_len = (buf_len as usize).min(1024 * 1024);
        let mut buf = vec![0u8; buf_len];

        let rc = libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return None;
        }

        let name_ptr = pwd.pw_name;
        if name_ptr.is_null() {
            return None;
        }

        let cstr = std::ffi::CStr::from_ptr(name_ptr);
        let s = cstr.to_string_lossy().trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

fn check_evdev_keyboard_access() -> Result<(), String> {
    let dir =
        std::fs::read_dir("/dev/input").map_err(|err| format!("/dev/input not readable: {err}"))?;

    let mut permission_denied = false;
    for entry in dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("event") {
            continue;
        }

        match evdev::Device::open(&path) {
            Ok(device) => {
                if device
                    .supported_keys()
                    .map(|keys| {
                        keys.contains(evdev::Key::KEY_A)
                            && keys.contains(evdev::Key::KEY_Z)
                            && keys.contains(evdev::Key::KEY_ENTER)
                    })
                    .unwrap_or(false)
                {
                    return Ok(());
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied = true;
            }
            Err(_) => {}
        }
    }

    if permission_denied {
        return Err(
            "No readable keyboard devices. Add your user to the 'input' group (then log out/in)."
                .to_string(),
        );
    }

    Err("No keyboard devices found under /dev/input".to_string())
}

fn check_uinput_access() -> Result<(), String> {
    use std::fs::OpenOptions;

    if !std::path::Path::new("/dev/uinput").exists() {
        return Err(
            "/dev/uinput not found (load the uinput kernel module: modprobe uinput)".to_string(),
        );
    }

    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
        .map(|_| ())
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                "Cannot open /dev/uinput. Configure udev permissions (and ensure ACLs do not block the 'input' group) then log out/in.".to_string()
            } else {
                format!("Cannot open /dev/uinput: {err}")
            }
        })
}

fn binary_in_path(binary: &str) -> bool {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let full = dir.join(binary);
            if full.is_file() {
                return true;
            }
        }
    }

    for dir in ["/usr/bin", "/usr/local/bin", "/bin"] {
        let full = std::path::Path::new(dir).join(binary);
        if full.is_file() {
            return true;
        }
    }

    false
}
