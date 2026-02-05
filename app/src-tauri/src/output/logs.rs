use std::collections::VecDeque;
use std::sync::RwLock;

use once_cell::sync::Lazy;
use tauri::{AppHandle, Emitter, Runtime};

static LOG_BUFFER: Lazy<RwLock<VecDeque<String>>> =
    Lazy::new(|| RwLock::new(VecDeque::with_capacity(512)));

pub fn push_log(line: impl Into<String>) {
    let mut buffer = LOG_BUFFER.write().expect("log buffer poisoned");
    if buffer.len() >= 512 {
        buffer.pop_front();
    }
    buffer.push_back(line.into());
}

pub fn snapshot() -> Vec<String> {
    LOG_BUFFER
        .read()
        .map(|buffer| buffer.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn broadcast_logs<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit("logs-updated", snapshot());
}

pub fn initialize<R: Runtime>(app: &AppHandle<R>) {
    push_log("Log viewer initialized");
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            broadcast_logs(&handle);
        }
    });
}
