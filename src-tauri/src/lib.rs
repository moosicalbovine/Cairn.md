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

#[cfg(debug_assertions)]
fn webdriver_library_root() -> Result<Option<std::path::PathBuf>, Box<dyn std::error::Error>> {
    if std::env::var("CAIRN_WEBDRIVER_MODE").as_deref() != Ok("1") {
        return Ok(None);
    }
    let root = std::env::var_os("CAIRN_WEBDRIVER_LIBRARY_ROOT")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .ok_or("CAIRN_WEBDRIVER_LIBRARY_ROOT is required in WebDriver mode")?;
    Ok(Some(root))
}

#[cfg(not(debug_assertions))]
fn webdriver_library_root() -> Result<Option<std::path::PathBuf>, Box<dyn std::error::Error>> {
    Ok(None)
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
            let webdriver_root = webdriver_library_root()?;
            let app_data_dir =
                if commands::performance::performance_mode() || webdriver_root.is_some() {
                    std::env::var_os("CAIRN_APP_DATA_DIR")
                        .filter(|value| !value.is_empty())
                        .map(std::path::PathBuf::from)
                        .ok_or("CAIRN_APP_DATA_DIR is required in isolated test mode")?
                } else {
                    app.path().app_local_data_dir()?
                };
            let mut library = LibraryService::open(&app_data_dir)?;
            if let Some(root) = webdriver_root {
                if library.snapshot()?.binding.is_none() {
                    library.bind_root(&root)?;
                }
            }
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
            commands::persistence::reload_document_from_disk,
            commands::performance::performance_mode,
            commands::performance::performance_scenario,
            commands::performance::performance_fixture_paths,
            commands::performance::mark_performance_ready,
            commands::performance::write_performance_report,
        ])
        .run(tauri::generate_context!())
        .expect("Cairn.md desktop core failed to start");
}
