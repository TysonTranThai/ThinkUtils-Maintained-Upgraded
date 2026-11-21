use serde::{Deserialize, Serialize};

#[cfg(not(target_os = "windows"))]
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub model: String,
    pub cpu: String,
    pub memory: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_system_info() -> ApiResponse<SystemInfo> {
    #[cfg(target_os = "windows")]
    {
        return windows_system_info();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let info = SystemInfo {
            hostname: get_hostname(),
            os: get_os_info(),
            kernel: get_kernel_version(),
            model: get_model(),
            cpu: get_cpu_info(),
            memory: get_memory_info(),
        };

        ApiResponse {
            success: true,
            data: Some(info),
            error: None,
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
struct WindowsInfo {
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    kernel: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    cpu: String,
    #[serde(default)]
    memory_bytes: f64,
}

#[cfg(target_os = "windows")]
fn windows_system_info() -> ApiResponse<SystemInfo> {
    // One CIM query rather than five. `Get-CimInstance` is available on every
    // supported Windows release and does not require administrator rights for
    // these classes.
    let script = r#"
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
$cs = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
$cp = Get-CimInstance Win32_ComputerSystemProduct -ErrorAction SilentlyContinue
$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue | Select-Object -First 1
$model = if ($cp -and $cp.Version) { $cp.Version } else { (@($cs.Manufacturer, $cs.Model) -join ' ').Trim() }
[pscustomobject]@{
    hostname = $env:COMPUTERNAME
    os = $os.Caption
    kernel = $os.Version
    model = $model
    cpu = if ($cpu) { $cpu.Name } else { 'Unknown CPU' }
    memory_bytes = [double]$cs.TotalPhysicalMemory
} | ConvertTo-Json -Compress
"#;

    match crate::platform::powershell(script)
        .and_then(|raw| serde_json::from_str::<WindowsInfo>(&raw).map_err(|e| e.to_string()))
    {
        Ok(info) => {
            let memory = if info.memory_bytes > 0.0 {
                format!("{:.1} GB", info.memory_bytes / 1024.0 / 1024.0 / 1024.0)
            } else {
                "Unknown".to_string()
            };

            ApiResponse {
                success: true,
                data: Some(SystemInfo {
                    hostname: non_empty(info.hostname, "Unknown"),
                    os: non_empty(info.os, "Windows"),
                    kernel: non_empty(info.kernel, "Unknown"),
                    model: non_empty(info.model, "Unknown"),
                    cpu: non_empty(info.cpu, "Unknown CPU"),
                    memory,
                }),
                error: None,
            }
        }
        Err(e) => {
            // CIM can be blocked by policy. Environment variables still give a
            // useful hostname, and the page can say which fields are missing
            // instead of failing as a whole.
            ApiResponse {
                success: true,
                data: Some(SystemInfo {
                    hostname: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Unknown".into()),
                    os: "Windows".to_string(),
                    kernel: "Unknown".to_string(),
                    model: "Unknown".to_string(),
                    cpu: "Unknown CPU".to_string(),
                    memory: "Unknown".to_string(),
                }),
                error: Some(format!("Could not query Windows system information: {}", e)),
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn non_empty(value: String, fallback: &str) -> String {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

#[cfg(not(target_os = "windows"))]
fn get_hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

#[cfg(not(target_os = "windows"))]
fn get_os_info() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("PRETTY_NAME="))
                .map(|line| {
                    line.trim_start_matches("PRETTY_NAME=")
                        .trim_matches('"')
                        .to_string()
                })
        })
        .unwrap_or_else(|| "Linux".to_string())
}

#[cfg(not(target_os = "windows"))]
fn get_kernel_version() -> String {
    Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

#[cfg(not(target_os = "windows"))]
fn get_model() -> String {
    std::fs::read_to_string("/sys/class/dmi/id/product_name")
        .ok()
        .map(|s| s.trim().to_string())
        .or_else(|| {
            std::fs::read_to_string("/sys/class/dmi/id/product_version")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "Unknown ThinkPad".to_string())
}

#[cfg(not(target_os = "windows"))]
fn get_cpu_info() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

#[cfg(not(target_os = "windows"))]
fn get_memory_info() -> String {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("MemTotal:"))
                .and_then(|line| {
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|kb| kb.parse::<u64>().ok())
                        .map(|kb| {
                            let gb = kb as f64 / 1024.0 / 1024.0;
                            format!("{:.1} GB", gb)
                        })
                })
        })
        .unwrap_or_else(|| "Unknown".to_string())
}
