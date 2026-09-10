use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const PERFORMANCE_MODE: &str = "CAIRN_PERF_MODE";
const PERFORMANCE_OUTPUT: &str = "CAIRN_PERF_OUTPUT";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPerformanceReport {
    document_open_ms: Vec<f64>,
    input_latency_ms: Vec<f64>,
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

#[cfg(test)]
mod tests {
    use super::validate_samples;

    #[test]
    fn performance_reports_require_twenty_valid_samples() {
        assert!(validate_samples(&vec![1.0; 20], "metric").is_ok());
        assert!(validate_samples(&vec![1.0; 19], "metric").is_err());
        let mut invalid = vec![1.0; 20];
        invalid[4] = f64::NAN;
        assert!(validate_samples(&invalid, "metric").is_err());
    }
}
