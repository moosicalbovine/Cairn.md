use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const PERFORMANCE_MODE: &str = "CAIRN_PERF_MODE";
const PERFORMANCE_OUTPUT: &str = "CAIRN_PERF_OUTPUT";
const PERFORMANCE_SCENARIO: &str = "CAIRN_PERF_SCENARIO";
const PERFORMANCE_LIBRARY_ROOT: &str = "CAIRN_PERF_LIBRARY_ROOT";
const PERFORMANCE_TRACKED_ROOT: &str = "CAIRN_PERF_TRACKED_ROOT";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPerformanceReport {
    document_open_ms: Vec<f64>,
    input_latency_ms: Vec<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceFixturePaths {
    library_root: PathBuf,
    tracked_root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSmokeReport {
    project_created: bool,
    tracked_import: bool,
    visual_edit: bool,
    source_mode: bool,
    autosave: bool,
    #[serde(default)]
    message: String,
}

fn performance_output() -> Result<PathBuf, String> {
    if !performance_mode() {
        return Err("Performance reporting is disabled".to_owned());
    }
    std::env::var_os(PERFORMANCE_OUTPUT)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "The performance output path is missing".to_owned())
}

fn ready_path(output: &Path) -> PathBuf {
    let mut value: OsString = output.as_os_str().to_owned();
    value.push(".ready");
    PathBuf::from(value)
}

fn validate_samples(samples: &[f64], name: &str) -> Result<(), String> {
    if samples.len() < 20
        || samples
            .iter()
            .any(|sample| !sample.is_finite() || *sample < 0.0)
    {
        return Err(format!(
            "{name} must contain at least 20 finite non-negative samples"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn performance_mode() -> bool {
    std::env::var(PERFORMANCE_MODE).is_ok_and(|value| value == "1")
}

#[tauri::command]
pub fn performance_scenario() -> &'static str {
    match std::env::var(PERFORMANCE_SCENARIO).as_deref() {
        Ok("idle") => "idle",
        Ok("installed") => "installed",
        _ => "full",
    }
}

fn fixture_path(name: &str) -> Result<PathBuf, String> {
    let value = std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required for the installed fixture"))?;
    absolute_fixture_path(value, name)
}

fn absolute_fixture_path(value: OsString, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{name} must be an absolute path"));
    }
    Ok(path)
}

#[tauri::command]
pub fn performance_fixture_paths() -> Result<PerformanceFixturePaths, String> {
    if !performance_mode() || performance_scenario() != "installed" {
        return Err("The installed fixture is disabled".to_owned());
    }
    let library_root = fixture_path(PERFORMANCE_LIBRARY_ROOT)?;
    let tracked_root = fixture_path(PERFORMANCE_TRACKED_ROOT)?;
    if library_root == tracked_root {
        return Err("Workspace fixture roots must be different".to_owned());
    }
    Ok(PerformanceFixturePaths {
        library_root,
        tracked_root,
    })
}

fn installed_checks_pass(report: &InstalledSmokeReport, original_source_unchanged: bool) -> bool {
    report.project_created
        && report.tracked_import
        && report.visual_edit
        && report.source_mode
        && report.autosave
        && original_source_unchanged
}

#[tauri::command]
pub fn mark_performance_ready() -> Result<(), String> {
    let output = performance_output()?;
    fs::write(ready_path(&output), b"ready").map_err(|error| error.to_string())
}

#[tauri::command]
pub fn write_performance_report(report: BrowserPerformanceReport) -> Result<(), String> {
    validate_samples(&report.document_open_ms, "documentOpenMs")?;
    validate_samples(&report.input_latency_ms, "inputLatencyMs")?;
    let output = performance_output()?;
    let value = serde_json::json!({
        "documentOpenMs": report.document_open_ms,
        "inputLatencyMs": report.input_latency_ms,
    });
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(output, bytes).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn write_installed_smoke_report(report: InstalledSmokeReport) -> Result<(), String> {
    if !performance_mode() || performance_scenario() != "installed" {
        return Err("Installed workflow reporting is disabled".to_owned());
    }
    let paths = performance_fixture_paths()?;
    let original_source_unchanged = fs::read(paths.tracked_root.join("tracked-proof.md"))
        .is_ok_and(|bytes| bytes == b"# Original tracked file remains unchanged.");
    let passed = installed_checks_pass(&report, original_source_unchanged);
    let output = performance_output()?;
    let value = serde_json::json!({
        "benchmark": "cairn-installed-workflow",
        "checks": {
            "projectCreated": report.project_created,
            "trackedImport": report.tracked_import,
            "originalSourceUnchanged": original_source_unchanged,
            "visualEdit": report.visual_edit,
            "sourceMode": report.source_mode,
            "autosave": report.autosave,
        },
        "passed": passed,
    });
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(&output, bytes).map_err(|error| error.to_string())?;
    fs::write(ready_path(&output), b"ready").map_err(|error| error.to_string())?;
    if passed {
        Ok(())
    } else {
        Err(if report.message.is_empty() {
            "Installed workflow checks failed".to_owned()
        } else {
            report.message
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{
        absolute_fixture_path, installed_checks_pass, validate_samples, InstalledSmokeReport,
    };

    #[test]
    fn performance_reports_require_twenty_valid_samples() {
        assert!(validate_samples(&[1.0; 20], "metric").is_ok());
        assert!(validate_samples(&[1.0; 19], "metric").is_err());
        let mut invalid = vec![1.0; 20];
        invalid[4] = f64::NAN;
        assert!(validate_samples(&invalid, "metric").is_err());
    }

    #[test]
    fn fixture_paths_must_be_absolute() {
        assert!(absolute_fixture_path(OsString::from("relative"), "fixture").is_err());
    }

    #[test]
    fn installed_report_requires_every_check_and_the_original_source() {
        let report = InstalledSmokeReport {
            project_created: true,
            tracked_import: true,
            visual_edit: true,
            source_mode: true,
            autosave: true,
            message: String::new(),
        };

        assert!(installed_checks_pass(&report, true));
        assert!(!installed_checks_pass(&report, false));
    }
}
