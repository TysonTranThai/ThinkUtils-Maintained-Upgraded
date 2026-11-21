//! System monitoring types and the platform dispatch.
//!
//! The shape of the payload is shared so the Monitor view is identical on both
//! platforms. Collection is not: Linux reads `/proc` and shells out to `df` and
//! `ps`, while Windows samples the native APIs in `monitor_windows`. Keeping
//! the Linux implementation in its own module means the Windows build never
//! compiles a `/proc` path.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemMonitor {
    pub cpu: CpuStats,
    pub memory: MemoryStats,
    pub disk: Vec<DiskStats>,
    pub network: Vec<NetworkStats>,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CpuStats {
    pub usage_percent: f64,
    pub cores: Vec<CoreStats>,
    pub load_avg: LoadAverage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CoreStats {
    pub core_id: usize,
    pub usage_percent: f64,
    pub frequency: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one_min: f64,
    pub five_min: f64,
    pub fifteen_min: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub usage_percent: f64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiskStats {
    pub device: String,
    pub mount_point: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub usage_percent: f64,
    pub filesystem: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkStats {
    pub interface: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

/// Collecting stats blocks, so it must never run inline on the IPC thread.
///
/// The Monitor view polls every 2s. On Linux the collection sleeps 200ms
/// between two `/proc/stat` samples and runs two subprocesses; on Windows it
/// sleeps the same 200ms between two native samples. Either way, doing that on
/// the IPC thread stalls the UI for a tenth of every poll.
#[tauri::command]
pub async fn get_system_monitor() -> ApiResponse<SystemMonitor> {
    #[cfg(target_os = "windows")]
    {
        return crate::monitor_windows::get_system_monitor().await;
    }

    #[cfg(not(target_os = "windows"))]
    {
        match tokio::task::spawn_blocking(crate::monitor_linux::collect_system_stats).await {
            Ok(Ok(monitor)) => ApiResponse {
                success: true,
                data: Some(monitor),
                error: None,
            },
            Ok(Err(e)) => ApiResponse {
                success: false,
                data: None,
                error: Some(e),
            },
            Err(e) => ApiResponse {
                success: false,
                data: None,
                error: Some(format!("Monitor collection failed to run: {}", e)),
            },
        }
    }
}
