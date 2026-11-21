//! Security page types and platform dispatch.
//!
//! The Linux backend controls ClamAV through `clamscan`/`freshclam` and the
//! package manager. Windows ships Microsoft Defender, so it has a completely
//! different backend in `security_windows`. The response shapes are shared so
//! the Security view can render either one.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityStatus {
    pub clamav_installed: bool,
    pub clamav_running: bool,
    pub database_version: String,
    pub last_update: String,
    pub definitions_count: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanResult {
    pub success: bool,
    pub scanned_files: u32,
    pub infected_files: u32,
    pub scan_time: String,
    pub threats: Vec<ThreatInfo>,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThreatInfo {
    pub file_path: String,
    pub threat_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallResponse {
    pub success: bool,
    pub message: String,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn get_security_status() -> Response<SecurityStatus> {
    #[cfg(target_os = "windows")]
    {
        return crate::security_windows::get_security_status().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::security_linux::get_security_status().await
    }
}

#[tauri::command]
pub async fn update_virus_definitions() -> Response<String> {
    #[cfg(target_os = "windows")]
    {
        return crate::security_windows::update_virus_definitions().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::security_linux::update_virus_definitions().await
    }
}

#[tauri::command]
pub async fn scan_path(path: String) -> Response<ScanResult> {
    #[cfg(target_os = "windows")]
    {
        return crate::security_windows::scan_path(path).await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::security_linux::scan_path(path).await
    }
}

#[tauri::command]
pub async fn quick_scan() -> Response<ScanResult> {
    #[cfg(target_os = "windows")]
    {
        return crate::security_windows::quick_scan().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::security_linux::quick_scan().await
    }
}

#[tauri::command]
pub async fn install_clamav() -> InstallResponse {
    #[cfg(target_os = "windows")]
    {
        return crate::security_windows::install_antivirus().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::security_linux::install_clamav().await
    }
}
