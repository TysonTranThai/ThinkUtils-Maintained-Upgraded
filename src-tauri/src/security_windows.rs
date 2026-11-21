//! Windows antivirus backend (Microsoft Defender).
//!
//! Defender exposes no native C API for status or scanning, but the
//! `Defender` PowerShell module is present on every supported Windows release
//! and is the documented interface. These calls are one-shot user actions, not
//! polling, so a PowerShell process is the right tool here.
//!
//! `Start-MpScan` does not report a scanned-file count, and Defender's
//! real-time protection cannot be toggled without tampering with a protected
//! service. Both are reported honestly instead of being guessed.

use crate::platform::{powershell, ps_single_quote};
use crate::security::{InstallResponse, Response, ScanResult, SecurityStatus, ThreatInfo};
use serde::Deserialize;
use std::time::Instant;

#[derive(Debug, Deserialize, Default)]
struct DefenderStatus {
    #[serde(default)]
    available: bool,
    #[serde(default)]
    running: bool,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    signature: String,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize, Default)]
struct DefenderUpdate {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    signature: String,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Deserialize, Default)]
struct ThreatJson {
    #[serde(default)]
    file: String,
    #[serde(default)]
    name: String,
}

fn status_script() -> &'static str {
    r#"
try {
    $s = Get-MpComputerStatus -ErrorAction Stop
    $updated = if ($s.AntivirusSignatureLastUpdated) {
        $s.AntivirusSignatureLastUpdated.ToString('yyyy-MM-dd HH:mm')
    } else { 'Unknown' }
    [pscustomobject]@{
        available = $true
        running = [bool]$s.RealTimeProtectionEnabled
        engine = [string]$s.AMEngineVersion
        signature = [string]$s.AntivirusSignatureVersion
        updated = [string]$updated
        error = ''
    } | ConvertTo-Json -Compress
} catch {
    [pscustomobject]@{
        available = $false
        running = $false
        engine = ''
        signature = ''
        updated = ''
        error = [string]$_.Exception.Message
    } | ConvertTo-Json -Compress
}
"#
}

pub async fn get_security_status() -> Response<SecurityStatus> {
    let result = tokio::task::spawn_blocking(|| powershell(status_script())).await;

    let raw = match result {
        Ok(Ok(raw)) => raw,
        Ok(Err(e)) => {
            return Response {
                success: false,
                data: None,
                error: Some(format!("Could not query Microsoft Defender: {}", e)),
            }
        }
        Err(e) => {
            return Response {
                success: false,
                data: None,
                error: Some(format!("Defender status task failed: {}", e)),
            }
        }
    };

    let status: DefenderStatus = serde_json::from_str(&raw).unwrap_or_default();

    Response {
        success: true,
        data: Some(SecurityStatus {
            clamav_installed: status.available,
            clamav_running: status.running,
            database_version: if status.engine.is_empty() {
                "Unknown".to_string()
            } else {
                status.engine
            },
            last_update: if status.updated.is_empty() {
                "Unknown".to_string()
            } else {
                status.updated
            },
            definitions_count: if status.signature.is_empty() {
                "Unknown".to_string()
            } else {
                status.signature
            },
        }),
        // A missing Defender module is not a failed query; it is a machine where
        // Defender is absent or replaced. The page explains that state.
        error: if status.error.is_empty() {
            None
        } else {
            Some(status.error)
        },
    }
}

pub async fn update_virus_definitions() -> Response<String> {
    let script = r#"
try {
    Update-MpSignature -ErrorAction Stop
    $s = Get-MpComputerStatus -ErrorAction SilentlyContinue
    [pscustomobject]@{
        ok = $true
        signature = [string]$s.AntivirusSignatureVersion
        error = ''
    } | ConvertTo-Json -Compress
} catch {
    [pscustomobject]@{
        ok = $false
        signature = ''
        error = [string]$_.Exception.Message
    } | ConvertTo-Json -Compress
}
"#;

    let result = tokio::task::spawn_blocking(move || powershell(script)).await;

    let raw = match result {
        Ok(Ok(raw)) => raw,
        Ok(Err(e)) => {
            return Response {
                success: false,
                data: None,
                error: Some(format!("Signature update failed: {}", e)),
            }
        }
        Err(e) => {
            return Response {
                success: false,
                data: None,
                error: Some(format!("Signature update task failed: {}", e)),
            }
        }
    };

    let update: DefenderUpdate = serde_json::from_str(&raw).unwrap_or_default();

    if update.ok {
        Response {
            success: true,
            data: Some(if update.signature.is_empty() {
                "Microsoft Defender signatures are up to date.".to_string()
            } else {
                format!(
                    "Microsoft Defender signatures updated to {}.",
                    update.signature
                )
            }),
            error: None,
        }
    } else {
        Response {
            success: false,
            data: None,
            error: Some(format!(
                "Signature update failed: {}. This usually means signature updates are managed by your organization or Windows Update.",
                if update.error.is_empty() {
                    "unknown error"
                } else {
                    update.error.as_str()
                }
            )),
        }
    }
}

/// Defender reports detections through `Get-MpThreatDetection`, not through
/// the scan command's output. The time filter keeps unrelated older detections
/// out of the result.
fn scan_script(scan_type: &str, path: Option<&str>) -> String {
    let target = path
        .map(|p| format!("-ScanPath '{}'", ps_single_quote(p)))
        .unwrap_or_default();

    format!(
        r#"
$start = [datetime]::UtcNow.AddSeconds(-5)
try {{
    Start-MpScan -ScanType {scan_type} {target} -ErrorAction Stop
    Start-Sleep -Seconds 2
    $threats = @(
        Get-MpThreatDetection -ErrorAction SilentlyContinue |
            Where-Object {{ $_.InitialDetectionTime -ge $start }} |
            Sort-Object InitialDetectionTime -Descending |
            Select-Object -First 20 |
            ForEach-Object {{
                [pscustomobject]@{{
                    file = [string](@($_.Resources)[0])
                    name = [string]$_.ThreatName
                }}
            }}
    )
    [pscustomobject]@{{ ok = $true; threats = $threats; error = '' }} |
        ConvertTo-Json -Compress -Depth 4
}} catch {{
    [pscustomobject]@{{ ok = $false; threats = @(); error = [string]$_.Exception.Message }} |
        ConvertTo-Json -Compress -Depth 4
}}
"#
    )
}

fn parse_threats(raw: &str) -> Vec<ThreatInfo> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };

    let Some(threats) = value.get("threats") else {
        return Vec::new();
    };

    let items = match threats {
        serde_json::Value::Array(items) => items.clone(),
        // Windows PowerShell 5.1 collapses a one-element array to an object.
        serde_json::Value::Object(_) => vec![threats.clone()],
        _ => return Vec::new(),
    };

    items
        .into_iter()
        .filter_map(|item| serde_json::from_value::<ThreatJson>(item).ok())
        .map(|threat| ThreatInfo {
            file_path: if threat.file.is_empty() {
                "Unknown location".to_string()
            } else {
                threat.file
            },
            threat_name: if threat.name.is_empty() {
                "Unknown threat".to_string()
            } else {
                threat.name
            },
        })
        .collect()
}

fn scan_failure(logs: Vec<String>, message: String) -> Response<ScanResult> {
    Response {
        success: false,
        data: Some(ScanResult {
            success: false,
            scanned_files: 0,
            infected_files: 0,
            scan_time: "0s".to_string(),
            threats: Vec::new(),
            logs,
            error: Some(message.clone()),
        }),
        error: Some(message),
    }
}

async fn run_scan(scan_type: &str, label: &str, path: Option<String>) -> Response<ScanResult> {
    let mut logs = vec![format!("Starting {} with Microsoft Defender...", label)];

    if let Some(path) = path.as_deref() {
        if !std::path::Path::new(path).exists() {
            logs.push(format!("✗ Path does not exist: {}", path));
            return scan_failure(logs, "Path does not exist".to_string());
        }
        logs.push(format!("Target: {}", path));
    }

    let script = scan_script(scan_type, path.as_deref());
    let start = Instant::now();
    let result = tokio::task::spawn_blocking(move || powershell(&script)).await;

    let raw = match result {
        Ok(Ok(raw)) => raw,
        Ok(Err(e)) => {
            logs.push(format!("✗ Scan failed: {}", e));
            logs.push("This action may require administrator approval.".to_string());
            return scan_failure(logs, format!("Scan failed: {}", e));
        }
        Err(e) => {
            logs.push(format!("✗ Scan task failed: {}", e));
            return scan_failure(logs, format!("Scan task failed: {}", e));
        }
    };

    let scan_time = format!("{:.2}s", start.elapsed().as_secs_f64());
    let ok = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| value.get("ok").and_then(|ok| ok.as_bool()))
        .unwrap_or(false);

    if !ok {
        let message = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(|e| e.as_str())
                    .map(str::to_string)
            })
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "Microsoft Defender did not complete the scan.".to_string());

        logs.push(format!("✗ Scan failed: {}", message));
        logs.push("This action may require administrator approval.".to_string());
        return scan_failure(logs, format!("Scan failed: {}", message));
    }

    let threats = parse_threats(&raw);
    let infected = threats.len() as u32;

    if infected > 0 {
        logs.push(format!("⚠ Microsoft Defender found {} threat(s)", infected));
        for threat in &threats {
            logs.push(format!("  {}: {}", threat.file_path, threat.threat_name));
        }
    } else {
        logs.push("✓ Scan complete: Microsoft Defender found no threats".to_string());
    }
    logs.push(
        "Microsoft Defender does not report a scanned-file count through Start-MpScan.".to_string(),
    );
    logs.push(format!("Scan completed in {}", scan_time));

    Response {
        success: true,
        data: Some(ScanResult {
            success: true,
            // Deliberately zero rather than invented. The view renders this as
            // "not reported" on Windows.
            scanned_files: 0,
            infected_files: infected,
            scan_time,
            threats,
            logs,
            error: None,
        }),
        error: None,
    }
}

pub async fn scan_path(path: String) -> Response<ScanResult> {
    run_scan("CustomScan", "custom scan", Some(path)).await
}

pub async fn quick_scan() -> Response<ScanResult> {
    run_scan("QuickScan", "quick scan", None).await
}

pub async fn install_antivirus() -> InstallResponse {
    InstallResponse {
        success: false,
        message: String::new(),
        logs: vec![
            "Microsoft Defender is built into Windows.".to_string(),
            "ThinkUtils does not install a second antivirus engine.".to_string(),
        ],
        error: Some(
            "Microsoft Defender is part of Windows and cannot be installed by ThinkUtils. Open Windows Security to check its status or install a different antivirus if your organization requires one."
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_threat_object_is_treated_as_a_list() {
        // Windows PowerShell 5.1 serialises a one-element array as an object.
        let raw = r#"{"ok":true,"threats":{"file":"C:\\tmp\\bad.exe","name":"Trojan:Win32/Test"},"error":""}"#;
        let threats = parse_threats(raw);
        assert_eq!(threats.len(), 1);
        assert_eq!(threats[0].threat_name, "Trojan:Win32/Test");
    }

    #[test]
    fn empty_threat_list_is_handled() {
        let raw = r#"{"ok":true,"threats":[],"error":""}"#;
        assert!(parse_threats(raw).is_empty());
    }

    #[test]
    fn malformed_json_yields_no_threats() {
        assert!(parse_threats("not json").is_empty());
    }

    #[test]
    fn custom_scan_path_is_quoted() {
        let script = scan_script("CustomScan", Some("C:\\Users\\o'brien\\Downloads"));
        assert!(script.contains("-ScanPath 'C:\\Users\\o''brien\\Downloads'"));
    }
}
