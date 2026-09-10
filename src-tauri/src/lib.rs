use serde::Serialize;
use tauri::Manager;

pub mod commands;
pub mod domain;
pub mod infrastructure;

use commands::library::LibraryState;
use domain::library::LibraryService;

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
        app: "Cairn.md",
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
        .plugin(tauri_plugin_dialog::init())
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
        .setup(|app| {
            let app_data_dir = app.path().app_local_data_dir()?;
            let library = LibraryService::open(&app_data_dir)?;
            app.manage(LibraryState::new(library));
            log::info!("Cairn.md desktop core started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            commands::library::probe_library_root,
            commands::library::bind_library_root,
            commands::library::preview_library_relink,
            commands::library::confirm_library_relink,
            commands::library::library_snapshot,
            commands::library::reconcile_library,
            commands::library::reconcile_library_if_requested,
            commands::library::create_project,
            commands::library::rename_project,
            commands::library::create_document,
            commands::library::read_document,
            commands::library::rename_document,
            commands::library::move_document,
            commands::library::delete_document,
            commands::import::import_markdown,
            commands::import::add_tracked_folder,
            commands::import::list_tracked_folders,
            commands::import::remove_tracked_folder,
            commands::import::list_tracked_folder_entries,
            commands::persistence::store_recovery_snapshot,
            commands::persistence::load_recovery_snapshot,
            commands::persistence::discard_recovery_snapshot,
            commands::persistence::save_document,
            commands::persistence::save_recovery_copy,
        ])
        .run(tauri::generate_context!())
        .expect("Cairn.md desktop core failed to start");
}
