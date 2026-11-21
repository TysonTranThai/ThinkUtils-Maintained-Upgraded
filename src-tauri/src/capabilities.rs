//! What this particular machine can actually do.
//!
//! The frontend must not present a control that the backend cannot honour. The
//! Linux build has always discovered this indirectly — a missing `/proc` file
//! or a module parameter produced an error after the click. That is acceptable
//! on a platform where every path is expected to exist, but on Windows almost
//! every ThinkPad-specific path is absent by design, so the UI needs an explicit
//! answer before it paints.
//!
//! Every feature is reported with one of five states:
//!
//! * `supported` — works without caveats.
//! * `partial` — some sub-features work; the detail names what does not.
//! * `monitoring_only` — readable, but writes are unavailable.
//! * `admin_required` — readable, but writes need an additional setup step.
//! * `unsupported` — not available on this OS/hardware; the detail says why.
//!
//! The Linux detection deliberately reuses the same probes the commands use
//! (`hardware_root`, `threshold_paths`, `detect_fingerprint_device`) so the
//! report cannot drift from the implementation.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Supported,
    Partial,
    MonitoringOnly,
    AdminRequired,
    Unsupported,
}

impl CapabilityStatus {
    /// Whether the feature is usable at all. `partial` and the two read-only
    /// states still get a nav entry; only `unsupported` is blocked.
    pub fn is_available(self) -> bool {
        !matches!(self, CapabilityStatus::Unsupported)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureCapability {
    pub id: String,
    pub title: String,
    pub status: CapabilityStatus,
    /// Why this state was chosen, in language suitable for a tooltip or a
    /// support card. Never empty.
    pub detail: String,
    /// The concrete backend: `thinkpad_acpi`, `windows-native`, `none`, ...
    pub backend: String,
}

impl FeatureCapability {
    fn new(
        id: &str,
        title: &str,
        status: CapabilityStatus,
        detail: impl Into<String>,
        backend: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            status,
            detail: detail.into(),
            backend: backend.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    /// `linux` or `windows`.
    pub platform: String,
    /// A display name such as `Ubuntu 26.04 LTS` or `Windows`.
    pub platform_name: String,
    pub is_thinkpad: bool,
    pub features: Vec<FeatureCapability>,
}

impl Capabilities {
    pub fn feature(&self, id: &str) -> Option<&FeatureCapability> {
        self.features.iter().find(|f| f.id == id)
    }
}

static IS_THINKPAD: OnceLock<bool> = OnceLock::new();

#[tauri::command]
pub fn get_capabilities() -> ApiResponse<Capabilities> {
    ApiResponse {
        success: true,
        data: Some(detect()),
        error: None,
    }
}

pub fn detect() -> Capabilities {
    #[cfg(target_os = "windows")]
    {
        windows_capabilities()
    }

    #[cfg(not(target_os = "windows"))]
    {
        linux_capabilities()
    }
}

fn is_thinkpad() -> bool {
    *IS_THINKPAD.get_or_init(detect_thinkpad)
}

#[cfg(target_os = "linux")]
fn detect_thinkpad() -> bool {
    let read = |path: &str| crate::hardware_root::read_to_string(path).unwrap_or_default();
    let hints = [
        read("/sys/class/dmi/id/product_version"),
        read("/sys/class/dmi/id/product_family"),
        read("/sys/class/dmi/id/board_name"),
        read("/sys/class/dmi/id/product_name"),
    ];

    hints.iter().any(|hint| {
        let lower = hint.to_lowercase();
        lower.contains("thinkpad")
    })
}

#[cfg(target_os = "windows")]
fn detect_thinkpad() -> bool {
    // Win32_ComputerSystemProduct.Version is the field Windows exposes the
    // marketing name in ("ThinkPad T14 Gen 3"). Model is often just the
    // machine type (21A0...), so both are checked.
    let script = r#"
$cs = Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue
$cp = Get-CimInstance Win32_ComputerSystemProduct -ErrorAction SilentlyContinue
"$($cs.Manufacturer)|$($cs.Model)|$($cp.Version)"
"#;

    let Ok(out) = crate::platform::powershell(script) else {
        return false;
    };

    out.to_lowercase().contains("thinkpad")
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn detect_thinkpad() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn platform_name() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=").map(|value| {
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string()
                })
            })
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Linux".to_string())
}

#[cfg(target_os = "windows")]
fn platform_name() -> String {
    // The System Info page queries the full version. This is just the family
    // label used by the support card, so it stays cheap and offline.
    "Windows".to_string()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_name() -> String {
    std::env::consts::OS.to_string()
}

#[cfg(target_os = "linux")]
fn linux_capabilities() -> Capabilities {
    use crate::capabilities::CapabilityStatus::*;
    use crate::platform::command_exists;

    let fan_present = crate::hardware_root::exists("/proc/acpi/ibm/fan");
    let fan_ready = crate::hardware_root::read_to_string("/proc/acpi/ibm/fan")
        .map(|content| {
            content
                .lines()
                .any(|l| l.trim_start().starts_with("commands:"))
        })
        .unwrap_or(false);
    let sensors_present = command_exists("sensors");
    let battery_present = crate::hardware_root::exists("/sys/class/power_supply/BAT0")
        || crate::hardware_root::exists("/sys/class/power_supply/BAT1");
    let thresholds = crate::battery::threshold_paths().is_some();
    let cpufreq = crate::hardware_root::exists("/sys/devices/system/cpu/cpu0/cpufreq");
    let governor = cpufreq;
    let turbo = crate::hardware_root::exists("/sys/devices/system/cpu/intel_pstate/no_turbo")
        || crate::hardware_root::exists("/sys/devices/system/cpu/cpufreq/boost");
    let ppd = command_exists("powerprofilesctl");
    let tlp = command_exists("tlp-stat");
    let clamav = command_exists("clamscan");
    let fprintd = command_exists("fprintd-list");
    let fingerprint_device = crate::biometrics::detect_fingerprint_device().found;
    let gnome = crate::gnome_extension::detect_is_gnome();

    let fan = if fan_ready {
        FeatureCapability::new(
            "fan",
            "Fan Control",
            Supported,
            "ThinkPad ACPI fan control is available through /proc/acpi/ibm/fan.",
            "thinkpad_acpi",
        )
    } else if fan_present {
        FeatureCapability::new(
            "fan",
            "Fan Control",
            AdminRequired,
            "The thinkpad_acpi module was loaded without fan_control=1. Enable it from the Fan Control page, then reboot or reload the module.",
            "thinkpad_acpi",
        )
    } else {
        FeatureCapability::new(
            "fan",
            "Fan Control",
            Unsupported,
            "No ThinkPad ACPI fan interface was found. This model may not expose fan control to Linux, or thinkpad_acpi is not loaded.",
            "none",
        )
    };

    let sensors = if sensors_present {
        FeatureCapability::new(
            "sensors",
            "Temperature Sensors",
            Supported,
            "lm-sensors is installed and provides temperature readings.",
            "lm-sensors",
        )
    } else {
        FeatureCapability::new(
            "sensors",
            "Temperature Sensors",
            Partial,
            "lm-sensors is not installed. Fan control still works, but the fan curve has no temperature source.",
            "none",
        )
    };

    let battery = if battery_present {
        FeatureCapability::new(
            "battery",
            "Battery",
            Supported,
            "Battery status is read from /sys/class/power_supply.",
            "sysfs",
        )
    } else {
        FeatureCapability::new(
            "battery",
            "Battery",
            Unsupported,
            "No battery was found under /sys/class/power_supply/BAT0 or BAT1.",
            "none",
        )
    };

    let battery_thresholds = if thresholds {
        FeatureCapability::new(
            "battery_thresholds",
            "Charge Thresholds",
            Supported,
            "This battery exposes charge start/stop controls through sysfs.",
            "sysfs",
        )
    } else {
        FeatureCapability::new(
            "battery_thresholds",
            "Charge Thresholds",
            Unsupported,
            "This battery exposes no charge threshold controls.",
            "none",
        )
    };

    let performance = if cpufreq || ppd || tlp {
        FeatureCapability::new(
            "performance",
            "Performance",
            Supported,
            "CPU frequency and/or platform power controls are available.",
            if cpufreq {
                "cpufreq"
            } else {
                "power-profiles-daemon/TLP"
            },
        )
    } else {
        FeatureCapability::new(
            "performance",
            "Performance",
            Unsupported,
            "No cpufreq governor and no supported power management tool were found.",
            "none",
        )
    };

    let performance_governor = if governor {
        FeatureCapability::new(
            "performance_governor",
            "CPU Governor",
            Supported,
            "The kernel exposes a cpufreq governor.",
            "cpufreq",
        )
    } else {
        FeatureCapability::new(
            "performance_governor",
            "CPU Governor",
            Unsupported,
            "This kernel exposes no cpufreq governor interface.",
            "none",
        )
    };

    let performance_turbo = if turbo {
        FeatureCapability::new(
            "performance_turbo",
            "Turbo Boost",
            Supported,
            "Intel P-state or cpufreq boost control is available.",
            "cpufreq",
        )
    } else {
        FeatureCapability::new(
            "performance_turbo",
            "Turbo Boost",
            Unsupported,
            "This CPU exposes no turbo/boost control file.",
            "none",
        )
    };

    let performance_power_profile = if ppd || tlp {
        FeatureCapability::new(
            "performance_power_profile",
            "Power Profile",
            Supported,
            "A platform power profile tool is installed.",
            if ppd { "power-profiles-daemon" } else { "TLP" },
        )
    } else {
        FeatureCapability::new(
            "performance_power_profile",
            "Power Profile",
            Unsupported,
            "Install power-profiles-daemon or TLP to switch platform power profiles.",
            "none",
        )
    };

    let security = if clamav {
        FeatureCapability::new(
            "security",
            "Security",
            Supported,
            "ClamAV is installed and can be controlled from ThinkUtils.",
            "clamav",
        )
    } else {
        FeatureCapability::new(
            "security",
            "Security",
            Partial,
            "ClamAV is not installed. The Security page can install it with your package manager.",
            "clamav",
        )
    };

    let security_antivirus = if clamav {
        FeatureCapability::new(
            "security_antivirus",
            "Antivirus",
            Supported,
            "ClamAV is installed.",
            "clamav",
        )
    } else {
        FeatureCapability::new(
            "security_antivirus",
            "Antivirus",
            Partial,
            "ClamAV is not installed; installation is offered on the Security page.",
            "clamav",
        )
    };

    let biometrics = if fprintd && fingerprint_device {
        FeatureCapability::new(
            "biometrics",
            "Fingerprint & Biometrics",
            Supported,
            "A fingerprint reader and fprintd are available.",
            "fprintd",
        )
    } else if fprintd {
        FeatureCapability::new(
            "biometrics",
            "Fingerprint & Biometrics",
            Unsupported,
            "fprintd is installed, but no supported fingerprint reader was detected.",
            "fprintd",
        )
    } else if fingerprint_device {
        FeatureCapability::new(
            "biometrics",
            "Fingerprint & Biometrics",
            Partial,
            "A fingerprint reader was detected. Install fprintd to enroll and verify fingerprints.",
            "fprintd",
        )
    } else {
        FeatureCapability::new(
            "biometrics",
            "Fingerprint & Biometrics",
            Unsupported,
            "No supported fingerprint reader was detected.",
            "none",
        )
    };

    let gnome_extension = if gnome {
        FeatureCapability::new(
            "gnome_extension",
            "GNOME Top Bar Indicator",
            Supported,
            "GNOME Shell was detected.",
            "gnome-shell",
        )
    } else {
        FeatureCapability::new(
            "gnome_extension",
            "GNOME Top Bar Indicator",
            Unsupported,
            "This desktop is not GNOME Shell.",
            "none",
        )
    };

    Capabilities {
        platform: "linux".to_string(),
        platform_name: platform_name(),
        is_thinkpad: is_thinkpad(),
        features: vec![
            FeatureCapability::new(
                "home",
                "Home",
                Supported,
                "System overview is available.",
                "shared",
            ),
            fan,
            sensors,
            battery,
            battery_thresholds,
            performance,
            performance_governor,
            performance_turbo,
            performance_power_profile,
            FeatureCapability::new(
                "monitor",
                "System Monitor",
                Supported,
                "CPU, memory, disk, network and process monitoring are available.",
                "procfs",
            ),
            FeatureCapability::new(
                "system",
                "System Info",
                Supported,
                "Hardware and OS information are available.",
                "dmi/procfs",
            ),
            security,
            security_antivirus,
            biometrics,
            gnome_extension,
            FeatureCapability::new(
                "mcp",
                "AI Integration",
                Supported,
                "The local MCP server is available. Hardware tools follow the same capabilities as the UI.",
                "rmcp",
            ),
            FeatureCapability::new(
                "settings",
                "Settings",
                Supported,
                "Settings import, export and reset are available.",
                "shared",
            ),
        ],
    }
}

#[cfg(target_os = "windows")]
fn windows_capabilities() -> Capabilities {
    use crate::capabilities::CapabilityStatus::*;

    let powercfg = crate::platform::command_exists("powercfg.exe")
        || crate::platform::command_exists("powercfg");

    Capabilities {
        platform: "windows".to_string(),
        platform_name: platform_name(),
        is_thinkpad: is_thinkpad(),
        features: vec![
            FeatureCapability::new(
                "home",
                "Home",
                Supported,
                "System overview is available.",
                "shared",
            ),
            FeatureCapability::new(
                "fan",
                "Fan Control",
                MonitoringOnly,
                "Windows does not expose the ThinkPad ACPI fan interface. Temperature readings are best-effort through ACPI thermal zones; fan speed control is not available.",
                "acpi-wmi",
            ),
            FeatureCapability::new(
                "sensors",
                "Temperature Sensors",
                MonitoringOnly,
                "Temperature readings come from ACPI thermal zones when the firmware exposes them. Fan RPM and per-core temperatures are usually unavailable.",
                "acpi-wmi",
            ),
            FeatureCapability::new(
                "battery",
                "Battery",
                Supported,
                "Battery status is read through the native Windows power API.",
                "GetSystemPowerStatus",
            ),
            FeatureCapability::new(
                "battery_thresholds",
                "Charge Thresholds",
                Unsupported,
                "Windows and the standard Lenovo driver do not expose charge thresholds through a documented API. Use Lenovo Vantage if your model supports it.",
                "none",
            ),
            FeatureCapability::new(
                "performance",
                "Performance",
                Partial,
                "Windows power plans can be switched. CPU governors and turbo boost are managed by Windows and are not exposed for per-setting control.",
                "powercfg",
            ),
            FeatureCapability::new(
                "performance_governor",
                "CPU Governor",
                Unsupported,
                "Windows does not expose Linux-style CPU governors. Use a power plan instead.",
                "none",
            ),
            FeatureCapability::new(
                "performance_turbo",
                "Turbo Boost",
                Unsupported,
                "Turbo boost is managed by the Windows power engine and firmware. ThinkUtils does not override it.",
                "none",
            ),
            FeatureCapability::new(
                "performance_power_profile",
                "Power Profile",
                if powercfg { Supported } else { Unsupported },
                if powercfg {
                    "Power plans are switched with powercfg."
                } else {
                    "powercfg.exe was not found, so power plans cannot be managed."
                },
                if powercfg { "powercfg" } else { "none" },
            ),
            FeatureCapability::new(
                "monitor",
                "System Monitor",
                Supported,
                "CPU, memory, disk, network and process monitoring use native Windows APIs.",
                "windows-native",
            ),
            FeatureCapability::new(
                "system",
                "System Info",
                Supported,
                "Hardware and OS information are read through CIM/WMI.",
                "cim",
            ),
            FeatureCapability::new(
                "security",
                "Security",
                Supported,
                "Microsoft Defender status and scans are available through PowerShell. Some actions may require administrator approval.",
                "microsoft-defender",
            ),
            FeatureCapability::new(
                "security_antivirus",
                "Antivirus",
                Supported,
                "Microsoft Defender is the supported antivirus backend on Windows.",
                "microsoft-defender",
            ),
            FeatureCapability::new(
                "biometrics",
                "Fingerprint & Biometrics",
                Unsupported,
                "Windows Hello enrollment and fingerprint management are owned by Windows Settings. ThinkUtils does not modify biometric credentials on Windows.",
                "windows-hello",
            ),
            FeatureCapability::new(
                "gnome_extension",
                "GNOME Top Bar Indicator",
                Unsupported,
                "GNOME Shell extensions are Linux-only.",
                "none",
            ),
            FeatureCapability::new(
                "mcp",
                "AI Integration",
                Unsupported,
                "The MCP server is not available on Windows in this release. Its tools expose fan, charge-threshold and cpufreq controls that have no Windows backend.",
                "none",
            ),
            FeatureCapability::new(
                "settings",
                "Settings",
                Supported,
                "Settings import, export and reset are available.",
                "shared",
            ),
        ],
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn linux_capabilities() -> Capabilities {
    Capabilities {
        platform: crate::platform::os_family().to_string(),
        platform_name: platform_name(),
        is_thinkpad: false,
        features: vec![FeatureCapability::new(
            "settings",
            "Settings",
            CapabilityStatus::Supported,
            "Settings are available.",
            "shared",
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_feature_has_a_reason_and_unique_id() {
        let caps = detect();
        let mut ids = std::collections::HashSet::new();

        for feature in &caps.features {
            assert!(!feature.detail.is_empty(), "{} has no detail", feature.id);
            assert!(!feature.backend.is_empty(), "{} has no backend", feature.id);
            assert!(
                ids.insert(feature.id.clone()),
                "duplicate id {}",
                feature.id
            );
        }

        assert!(caps.feature("settings").is_some());
    }

    #[test]
    fn only_unsupported_is_blocked() {
        assert!(CapabilityStatus::Supported.is_available());
        assert!(CapabilityStatus::Partial.is_available());
        assert!(CapabilityStatus::MonitoringOnly.is_available());
        assert!(CapabilityStatus::AdminRequired.is_available());
        assert!(!CapabilityStatus::Unsupported.is_available());
    }

    #[test]
    fn status_serializes_as_snake_case_for_the_frontend() {
        let json = serde_json::to_string(&CapabilityStatus::MonitoringOnly).unwrap();
        assert_eq!(json, "\"monitoring_only\"");
    }
}
