# Push-to-Talk STT

Initial scaffolding for the Push-to-Talk speech-to-text desktop application described in the PRD.

## Structure

- `app/`: Frontend (React + TypeScript + Vite) and Tauri configuration.
- `app/src-tauri/`: Rust backend with module stubs for audio, ASR, VAD, autoclean, models, and output pipelines. `tauri.conf.json` lives here so Windows clones don't need symlink support.
- `ci/`: Placeholder automation scripts for native dependency builds and signing.

## Windows setup

1. Install the [Microsoft Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the *Desktop development with C++* workload, and ensure the Windows 10/11 SDK option is checked.
2. Install the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section) (Tauri uses it to render the UI).
3. Install [rustup](https://rustup.rs/) and run `rustup default stable` so a recent stable toolchain is available.
4. Install the [LLVM toolchain](https://releases.llvm.org/download.html) (or `winget install LLVM.LLVM`) so `libclang.dll` and `llvm-nm.exe` are available. After installing:  
   - PowerShell (current session):  
     ```
     $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
     $env:PATH = "$env:PATH;C:\Program Files\LLVM\bin"
     ```
   - To persist between sessions:  
     ```
     [Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")
     [Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";C:\Program Files\LLVM\bin", "User")
     ```
     Alternatively set `NM_PATH` directly: `[Environment]::SetEnvironmentVariable("NM_PATH", "C:\Program Files\LLVM\bin\llvm-nm.exe", "User")`.
5. Install [MSYS2](https://www.msys2.org/) for the GNU build utilities (`libtoolize`, `aclocal`, `automake`, `autoconf`, `pkg-config`, `m4`). In the MSYS2 **UCRT64** shell run:  
   ```
   pacman -Syu --noconfirm          # repeat if prompted to restart shell
   pacman -S --needed --noconfirm mingw-w64-ucrt-x86_64-toolchain mingw-w64-ucrt-x86_64-libtool mingw-w64-ucrt-x86_64-pkg-config
   pacman -S --needed --noconfirm autoconf automake
   ```  
   Then, from PowerShell:  
   ```powershell
   $msysUsr  = 'C:\msys64\usr\bin'
   $msysUcrt = 'C:\msys64\ucrt64\bin'
   $env:PATH = "$msysUcrt;$msysUsr;$env:PATH"
   [Environment]::SetEnvironmentVariable('PATH', "$msysUcrt;$msysUsr;$env:PATH", 'User')
   [Environment]::SetEnvironmentVariable('MSYS2_SHELL', 'C:\msys64\usr\bin\bash.exe', 'User')
   ```  
   (Open a new PowerShell window and verify `where libtoolize` works.)
6. Install [Node.js 18+](https://nodejs.org/) and enable Corepack (`corepack enable`) or install Yarn globally (`npm install -g yarn`).
7. Inside `app/`, run `yarn install` to fetch the frontend dependencies.

Once the prerequisites are installed you can iterate entirely from PowerShell or Windows Terminal:

- `yarn tauri dev` (or `cargo tauri dev`) launches the dev build with hot reload for the React frontend and Rust backend.
- `yarn tauri build` produces a signed bundle (MSI/MSIX by default) that you can double-click to install.
- When making iterative changes, leave `yarn tauri dev` running; edits in `app/src` or `app/src-tauri` will rebuild automatically.
- If the Rust build fails because `yarn` didn't generate the Vite assets yet, run `yarn build` once or restart `yarn tauri dev`.
  - WebRTC audio processing is part of the default feature set, so the MSYS2 binaries above are required on Windows. If you need to temporarily bypass it (e.g., before MSYS2 is configured) run `yarn tauri dev --no-default-features --features audio,hud,models,real-audio,asr-sherpa,llama-polish,vad-silero`.

## Development (planned cross-platform)

1. Install the Rust toolchain (Rust 1.78+) and Tauri prerequisites.
2. Install Node dependencies with `yarn install`.
3. Run the combined dev environment with `yarn tauri dev`.

The backend currently exposes foundational settings management and IPC hooks; audio/ASR pipelines are stubbed pending native integration.

## Linux (Wayland) setup

On Wayland, applications cannot reliably capture global hotkeys or inject keystrokes through the compositor.
This project uses Linux's kernel input devices for a compositor-agnostic workflow:

- **Global hotkeys**: reads `/dev/input/event*` (requires access via the `input` group)
- **Paste into any normal text field**: creates a virtual keyboard via `/dev/uinput` to send the paste shortcut

### Recommended (in-app one-click)

1. Start the app.
2. Open **Settings** → **Linux Setup**.
3. Click **Enable (admin)**.
4. **Log out and back in** (required for group membership to take effect).

### Manual setup

1. Install clipboard tooling:
   - Debian/Ubuntu: `sudo apt install wl-clipboard`
   - Fedora: `sudo dnf install wl-clipboard`
   - Arch: `sudo pacman -S wl-clipboard`

2. Add your user to the `input` group:

```bash
sudo usermod -a -G input "$USER"
```

3. Allow `/dev/uinput` for the `input` group (udev rule):

```bash
sudo tee /etc/udev/rules.d/99-pushtotalk-uinput.rules >/dev/null <<'EOF'
KERNEL=="uinput", MODE="0660", GROUP="input"
EOF

sudo modprobe uinput || true
sudo udevadm control --reload-rules
sudo udevadm trigger --name-match=uinput || true
```

4. Log out and back in.

### Security note

Membership in the `input` group and access to `/dev/uinput` allows reading global key events and injecting input.
Only enable this on machines you trust.
