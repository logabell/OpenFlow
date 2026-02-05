#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

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
use tauri::{AppHandle, Manager};
use tracing::metadata::LevelFilter;

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

    // Re-register hotkey if the mode or hotkey bindings have changed
    core::hotkeys::reregister(&app).await?;

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
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| crate::core::linux_setup::enable_permissions_for_current_user())
            .await
            .map_err(|err| tauri::Error::from(anyhow!(err.to_string())))?
            .map_err(tauri::Error::from)?;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(tauri::Error::from(anyhow!(
            "linux_enable_permissions is only supported on Linux"
        )))
    }
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
    let filter = std::env::var("STT_LOG")
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
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            register_hotkeys,
            unregister_hotkeys,
            linux_permissions_status,
            linux_enable_permissions,
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
            output::tray::initialize(app)?;
            if let Some(state) = app.try_state::<AppState>() {
                let handle = app.handle();
                state.initialize_models(&handle)?;
                if let Err(error) = state.initialize_pipeline(&handle) {
                    tracing::warn!("Failed to initialize pipeline: {error:?}");
                }
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
