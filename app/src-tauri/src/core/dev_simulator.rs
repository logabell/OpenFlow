use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::time::interval;
use tracing::warn;

use crate::core::app_state::AppState;

pub fn start(app: &AppHandle) {
    if std::env::var("STT_DISABLE_DEV_SIM").is_ok() {
        return;
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(20));
        // tokio intervals tick immediately on the first await; skip it so we don't
        // inject dev transcripts right at startup.
        ticker.tick().await;
        let samples = [
            "Testing push to talk simulation.",
            "Dictation pipeline dev harness message.",
            "Performance diagnostics sample output.",
        ];
        let mut index = 0usize;

        loop {
            ticker.tick().await;
            let text = samples[index % samples.len()];
            let latency = 1500 + ((index % 4) as u64) * 180;
            let cpu = 55.0 + ((index % 5) as f32) * 8.5;
            index = index.wrapping_add(1);

            let state = app_handle.state::<AppState>();
            if let Err(error) = state.simulate_transcription(&app_handle, text, latency, cpu) {
                warn!("dev simulation failed: {error:?}");
            }
        }
    });
}
