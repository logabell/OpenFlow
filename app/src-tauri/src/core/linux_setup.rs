use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxPermissionsStatus {
    pub supported: bool,
    pub wayland_session: bool,
    pub xdg_runtime_dir_available: bool,
    pub evdev_readable: bool,
    pub uinput_writable: bool,
    pub wl_copy_available: bool,
    pub wl_paste_available: bool,
    pub pkexec_available: bool,
    pub setfacl_available: bool,
    pub details: Vec<String>,
}

pub fn permissions_status() -> LinuxPermissionsStatus {
    #[cfg(not(target_os = "linux"))]
    {
        return LinuxPermissionsStatus {
            supported: false,
            wayland_session: false,
            xdg_runtime_dir_available: false,
            evdev_readable: false,
            uinput_writable: false,
            wl_copy_available: false,
            wl_paste_available: false,
            pkexec_available: false,
            setfacl_available: false,
            details: vec!["Linux permissions check is only available on Linux".to_string()],
        };
    }

    #[cfg(target_os = "linux")]
    {
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
        if !xdg_runtime_dir_available {
            details.push("Missing XDG_RUNTIME_DIR (Wayland clipboard may not work)".to_string());
        }

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

        let wl_copy_available = binary_in_path("wl-copy");
        if !wl_copy_available {
            details.push("Missing wl-copy (install wl-clipboard)".to_string());
        }

        let wl_paste_available = binary_in_path("wl-paste");
        if !wl_paste_available {
            details.push("Missing wl-paste (install wl-clipboard)".to_string());
        }

        let pkexec_available = binary_in_path("pkexec");
        if !pkexec_available {
            details.push("Missing pkexec (install polkit)".to_string());
        }

        let setfacl_available = binary_in_path("setfacl");
        if !setfacl_available {
            details.push("Missing setfacl (install acl)".to_string());
        }

        LinuxPermissionsStatus {
            supported: true,
            wayland_session,
            xdg_runtime_dir_available,
            evdev_readable,
            uinput_writable,
            wl_copy_available,
            wl_paste_available,
            pkexec_available,
            setfacl_available,
            details,
        }
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub fn enable_permissions_for_current_user() -> anyhow::Result<()> {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() {
        anyhow::bail!("Could not determine current user (USER env var missing)");
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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
