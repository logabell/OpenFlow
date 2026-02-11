mod asr;
mod audio;
mod core;
mod llm;
mod models;
mod output;
mod vad;

use anyhow::anyhow;
use audio::{list_input_devices, AudioDeviceInfo};
use core::{app_state::AppState, pipeline::OutputMode, settings::FrontendSettings};
use models::ModelAsset;
use tauri::{image::Image, include_image, WebviewWindowBuilder};
use tauri::{AppHandle, Manager};
use tracing::metadata::LevelFilter;

const APP_ICON: Image<'_> = include_image!("./icons/32x32.png");

#[tauri::command]
async fn get_settings(state: tauri::State<'_, AppState>) -> tauri::Result<FrontendSettings> {
    state.settings_manager().read_frontend().map_err(Into::into)
}

#[tauri::command]
async fn update_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    settings: FrontendSettings,
) -> tauri::Result<()> {
    state
        .settings_manager()
        .write_frontend(settings)
        .map_err(tauri::Error::from)?;

    let fresh = state
        .settings_manager()
        .read_frontend()
        .map_err(tauri::Error::from)?;

    state
        .configure_pipeline(Some(&app), &fresh)
        .map_err(tauri::Error::from)?;

    state.sync_hud_overlay_mode(&app);

    // Warm the selected ASR model in the background so the next dictation starts instantly.
    state.kickoff_asr_warmup(&app);

    // Re-register hotkey if the mode or hotkey bindings have changed
    core::hotkeys::reregister(&app).await?;

    Ok(())
}

#[tauri::command]
async fn hud_ready(app: AppHandle, state: tauri::State<'_, AppState>) -> tauri::Result<()> {
    state.replay_hud_state(&app);
    Ok(())
}

#[tauri::command]
async fn register_hotkeys(app: AppHandle) -> tauri::Result<()> {
    core::hotkeys::register(&app).await?;
    Ok(())
}

#[tauri::command]
async fn unregister_hotkeys(app: AppHandle) -> tauri::Result<()> {
    core::hotkeys::unregister(&app).await?;
    Ok(())
}

#[tauri::command]
async fn linux_permissions_status() -> tauri::Result<core::linux_setup::LinuxPermissionsStatus> {
    Ok(core::linux_setup::permissions_status())
}

#[tauri::command]
async fn linux_enable_permissions() -> tauri::Result<()> {
    tokio::task::spawn_blocking(|| crate::core::linux_setup::enable_permissions_for_current_user())
        .await
        .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?
        .map_err(tauri::Error::from)?;
    Ok(())
}

#[tauri::command]
async fn gnome_hud_extension_status() -> tauri::Result<core::linux_setup::GnomeHudExtensionStatus> {
    Ok(core::linux_setup::gnome_hud_extension_status())
}

#[tauri::command]
async fn gnome_hud_extension_install() -> tauri::Result<core::linux_setup::GnomeHudExtensionStatus>
{
    tokio::task::spawn_blocking(|| crate::core::linux_setup::install_gnome_hud_extension())
        .await
        .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?
        .map_err(tauri::Error::from)
}

#[tauri::command]
async fn check_for_updates(force: Option<bool>) -> tauri::Result<core::updater::UpdateCheckResult> {
    let force = force.unwrap_or(false);
    tokio::task::spawn_blocking(move || crate::core::updater::check_for_updates(force))
        .await
        .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?
        .map_err(tauri::Error::from)
}

#[tauri::command]
async fn download_update(
    app: AppHandle,
    force: Option<bool>,
) -> tauri::Result<core::updater::DownloadedUpdate> {
    let force = force.unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        crate::core::updater::download_update_with_progress(force, |progress| {
            crate::core::events::emit_update_download_progress(&app, progress);
        })
    })
    .await
    .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?
    .map_err(tauri::Error::from)
}

#[tauri::command]
async fn apply_update(app: AppHandle, tarball_path: String) -> tauri::Result<()> {
    tokio::task::spawn_blocking(move || {
        crate::core::updater::apply_update_with_pkexec_with_progress(&tarball_path, |progress| {
            crate::core::events::emit_update_apply_progress(&app, progress);
        })
    })
    .await
    .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?
    .map_err(tauri::Error::from)
}

#[tauri::command]
async fn quit_app(app: AppHandle) -> tauri::Result<()> {
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn restart_app(app: AppHandle) -> tauri::Result<()> {
    let candidates = [
        "/opt/openflow/openflow",
        "/usr/local/bin/openflow",
        "openflow",
    ];

    let mut errors: Vec<String> = Vec::new();
    for candidate in candidates {
        if candidate.contains('/') && !std::path::Path::new(candidate).is_file() {
            continue;
        }

        match std::process::Command::new(candidate)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {
                app.exit(0);
                return Ok(());
            }
            Err(err) => {
                errors.push(format!("{candidate}: {err}"));
            }
        }
    }

    Err(tauri::Error::from(anyhow!(
        "Failed to restart app. {}",
        if errors.is_empty() {
            "No restart candidates found.".to_string()
        } else {
            errors.join("; ")
        }
    )))
}

#[tauri::command]
async fn begin_dictation(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    show_overlay: Option<bool>,
) -> tauri::Result<()> {
    match show_overlay {
        Some(show_overlay) => state.start_session_with_overlay(&app, show_overlay),
        None => state.start_session(&app),
    }
    Ok(())
}

#[tauri::command]
async fn mark_dictation_processing(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> tauri::Result<()> {
    state.mark_processing(&app);
    Ok(())
}

#[tauri::command]
async fn complete_dictation(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> tauri::Result<()> {
    state.complete_session(&app);
    Ok(())
}

#[tauri::command]
async fn list_models(state: tauri::State<'_, AppState>) -> tauri::Result<Vec<ModelAsset>> {
    let manager_arc = state.model_manager();
    let manager = manager_arc
        .lock()
        .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?;
    Ok(manager.assets().into_iter().cloned().collect())
}

#[tauri::command]
async fn install_model_asset(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
) -> tauri::Result<()> {
    state
        .queue_model_download(&app, &name)
        .map_err(tauri::Error::from)
}

#[tauri::command]
async fn uninstall_model_asset(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
) -> tauri::Result<()> {
    state
        .uninstall_model(&app, &name)
        .map_err(tauri::Error::from)
}

#[tauri::command]
async fn list_audio_devices() -> tauri::Result<Vec<AudioDeviceInfo>> {
    Ok(list_input_devices())
}

#[tauri::command]
async fn secure_field_blocked(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> tauri::Result<()> {
    state.secure_blocked(&app);
    Ok(())
}

#[tauri::command]
async fn set_output_mode(state: tauri::State<'_, AppState>, mode: OutputMode) -> tauri::Result<()> {
    state.set_output_mode(mode).map_err(tauri::Error::from)?;
    Ok(())
}

#[cfg(debug_assertions)]
#[tauri::command]
async fn get_logs() -> Vec<String> {
    crate::output::logs::snapshot()
}

fn setup_logging() {
    let filter = std::env::var("OPENFLOW_LOG")
        .or_else(|_| std::env::var("STT_LOG"))
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(LevelFilter::INFO);

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_target(false)
        .compact()
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}

fn main() {
    setup_logging();

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            hud_ready,
            register_hotkeys,
            unregister_hotkeys,
            linux_permissions_status,
            linux_enable_permissions,
            gnome_hud_extension_status,
            gnome_hud_extension_install,
            check_for_updates,
            download_update,
            apply_update,
            quit_app,
            restart_app,
            begin_dictation,
            mark_dictation_processing,
            complete_dictation,
            secure_field_blocked,
            set_output_mode,
            list_models,
            install_model_asset,
            uninstall_model_asset,
            list_audio_devices,
            #[cfg(debug_assertions)]
            get_logs
        ])
        .setup(|app| {
            // Create the main window manually so we can attach an icon at build time.
            // Some Linux window managers ignore `set_icon` if applied after window creation,
            // and Wayland shells generally rely on a .desktop entry for taskbar/dock icons.
            if app.get_webview_window("main").is_none() {
                if let Some(config) = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|w| w.label == "main")
                    .cloned()
                {
                    let _ = WebviewWindowBuilder::from_config(app.handle(), &config)
                        .and_then(|builder| builder.icon(APP_ICON))
                        .and_then(|builder| builder.build());
                }
            } else if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_icon(APP_ICON);
            }

            output::tray::initialize(app)?;
            if let Some(state) = app.try_state::<AppState>() {
                let handle = app.handle();
                state.initialize_models(&handle)?;
                if let Err(error) = state.initialize_pipeline(&handle) {
                    tracing::warn!("Failed to initialize pipeline: {error:?}");
                }
                state.sync_hud_overlay_mode(&handle);

                // Always start ASR warmup on launch (non-blocking).
                state.kickoff_asr_warmup(&handle);
                #[cfg(debug_assertions)]
                {
                    crate::output::logs::initialize(&handle);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
