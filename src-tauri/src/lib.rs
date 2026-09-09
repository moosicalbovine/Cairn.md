use serde::Serialize;
use tauri::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthResponse {
    pub app: &'static str,
    pub version: &'static str,
    pub status: HealthStatus,
}

pub fn health_payload() -> HealthResponse {
    HealthResponse {
        app: "NoteMD",
        version: env!("CARGO_PKG_VERSION"),
        status: HealthStatus::Ok,
    }
}

#[tauri::command]
fn health() -> HealthResponse {
    health_payload()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|_app| {
            log::info!("NoteMD desktop core started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![health])
        .run(tauri::generate_context!())
        .expect("NoteMD desktop core failed to start");
}
