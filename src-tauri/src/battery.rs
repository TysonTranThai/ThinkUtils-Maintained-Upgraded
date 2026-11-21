use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const BAT0_PATH: &str = "/sys/class/power_supply/BAT0";
const BAT1_PATH: &str = "/sys/class/power_supply/BAT1";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatteryInfo {
    pub name: String,
    pub status: String,
    pub capacity: u8,
    pub health: u8,
    pub cycles: u32,
    pub voltage: f32,
    pub current: f32,
    pub power: f32,
    pub energy_now: f32,
    pub energy_full: f32,
    pub energy_design: f32,
    pub technology: String,
    pub manufacturer: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatteryThresholds {
    pub start: u8,
    pub stop: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_battery_info() -> ApiResponse<Vec<BatteryInfo>> {
    #[cfg(target_os = "windows")]
    {
        return windows_battery::get_battery_info();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut batteries = Vec::new();

        for (index, bat_path) in [BAT0_PATH, BAT1_PATH].iter().enumerate() {
            if Path::new(bat_path).exists() {
                match read_battery_info(bat_path, index) {
                    Ok(info) => batteries.push(info),
                    Err(e) => println!("[Battery] Error reading {}: {}", bat_path, e),
                }
            }
        }

        if batteries.is_empty() {
            return ApiResponse {
                success: false,
                data: None,
                error: Some("No batteries found".to_string()),
            };
        }

        ApiResponse {
            success: true,
            data: Some(batteries),
            error: None,
        }
    }
}

fn read_battery_info(path: &str, index: usize) -> Result<BatteryInfo, String> {
    let read_file = |file: &str| -> Result<String, String> {
        fs::read_to_string(format!("{}/{}", path, file))
            .map(|s| s.trim().to_string())
            .map_err(|e| e.to_string())
    };

    let read_u32 = |file: &str| -> u32 {
        read_file(file)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    };

    let read_f32 = |file: &str| -> f32 {
        read_file(file)
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .map(|v| v / 1_000_000.0) // Convert from µV/µA to V/A
            .unwrap_or(0.0)
    };

    let capacity = read_u32("capacity") as u8;
    let energy_now = read_f32("energy_now");
    let energy_full = read_f32("energy_full");
    let energy_design = read_f32("energy_full_design");

    let health = if energy_design > 0.0 {
        ((energy_full / energy_design) * 100.0) as u8
    } else {
        100
    };

    let voltage = read_f32("voltage_now");

    // Read power_now directly (µW -> W) if available, or compute from voltage * current
    let power = if let Ok(p_str) = read_file("power_now") {
        p_str.parse::<f32>().map(|v| v / 1_000_000.0).unwrap_or(0.0)
    } else {
        let current = read_f32("current_now");
        voltage * current
    };

    let current = if let Ok(c_str) = read_file("current_now") {
        c_str.parse::<f32>().map(|v| v / 1_000_000.0).unwrap_or(0.0)
    } else if voltage > 0.0 {
        power / voltage
    } else {
        0.0
    };

    Ok(BatteryInfo {
        name: format!("BAT{}", index),
        status: read_file("status").unwrap_or_else(|_| "Unknown".to_string()),
        capacity,
        health,
        cycles: read_u32("cycle_count"),
        voltage,
        current,
        power,
        energy_now,
        energy_full,
        energy_design,
        technology: read_file("technology").unwrap_or_else(|_| "Unknown".to_string()),
        manufacturer: read_file("manufacturer").unwrap_or_else(|_| "Unknown".to_string()),
    })
}

/// Attribute names for the charge thresholds, most-standard first.
///
/// `charge_control_*` is the generic kernel power-supply API and works beyond
/// ThinkPads. `charge_*_threshold` is the older thinkpad_acpi-specific spelling.
///
/// On a ThinkPad BOTH exist and report the same value, but they are separate
/// sysfs files — so a chmod on one does not affect the other. That is exactly
/// how this broke: permissions.rs granted access to the legacy pair while
/// battery.rs wrote the standard pair, so "Grant Permissions" never made battery
/// thresholds writable and every change fell through to a password prompt.
const THRESHOLD_ATTRS: &[(&str, &str)] = &[
    (
        "charge_control_start_threshold",
        "charge_control_end_threshold",
    ),
    ("charge_start_threshold", "charge_stop_threshold"),
];

/// The threshold file pair this machine actually exposes.
///
/// Returns the first pair where both files exist. Every caller must go through
/// here — the duplication between modules is what allowed them to disagree.
pub fn threshold_paths() -> Option<(String, String)> {
    THRESHOLD_ATTRS.iter().find_map(|(start, stop)| {
        let start_path = format!("{}/{}", BAT0_PATH, start);
        let stop_path = format!("{}/{}", BAT0_PATH, stop);
        (Path::new(&start_path).exists() && Path::new(&stop_path).exists())
            .then_some((start_path, stop_path))
    })
}

#[tauri::command]
pub fn get_battery_thresholds() -> ApiResponse<BatteryThresholds> {
    #[cfg(target_os = "windows")]
    {
        return ApiResponse {
            success: false,
            data: None,
            error: Some(
                "Charge thresholds are not exposed by Windows on this machine. Use Lenovo Vantage if your model supports them."
                    .to_string(),
            ),
        };
    }

    #[cfg(not(target_os = "windows"))]
    {
        let (start_path, stop_path) = match threshold_paths() {
            Some(pair) => pair,
            None => {
                // Preserve the previous defaults so callers that ignore `success`
                // keep behaving as before.
                return ApiResponse {
                    success: true,
                    data: Some(BatteryThresholds {
                        start: 0,
                        stop: 100,
                    }),
                    error: None,
                };
            }
        };

        let start = fs::read_to_string(&start_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        let stop = fs::read_to_string(&stop_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(100);

        ApiResponse {
            success: true,
            data: Some(BatteryThresholds { start, stop }),
            error: None,
        }
    }
}

/// Whether the new start threshold must be written before the new stop threshold.
///
/// The firmware rejects any write that would leave start >= stop, even
/// transiently, so the order depends on where the new stop lands relative to the
/// start already on the hardware.
///
/// Writing stop first leaves the intermediate state (current_start, new_stop).
/// That is only safe while current_start < new_stop. The boundary case matters:
/// at new_stop == current_start the intermediate state is (X, X), which is
/// start == stop and is rejected — hence >=, not >.
fn write_start_first(current_start: u8, new_stop: u8) -> bool {
    new_stop <= current_start
}

#[tauri::command]
pub async fn set_battery_thresholds(start: u8, stop: u8) -> ApiResponse<String> {
    #[cfg(target_os = "windows")]
    {
        let _ = (start, stop);
        return ApiResponse {
            success: false,
            data: None,
            error: Some(
                "ThinkUtils cannot change battery charge thresholds on Windows. Use Lenovo Vantage if your model supports them."
                    .to_string(),
            ),
        };
    }

    #[cfg(not(target_os = "windows"))]
    {
        if start >= stop {
            return ApiResponse {
                success: false,
                data: None,
                error: Some("Start threshold must be less than stop threshold".to_string()),
            };
        }

        if start > 100 || stop > 100 {
            return ApiResponse {
                success: false,
                data: None,
                error: Some("Thresholds must be between 0 and 100".to_string()),
            };
        }

        let (start_path, stop_path) = match threshold_paths() {
            Some(pair) => pair,
            None => {
                return ApiResponse {
                    success: false,
                    data: None,
                    error: Some(
                        "This machine exposes no battery charge threshold controls.".to_string(),
                    ),
                }
            }
        };

        // get_battery_thresholds() has no failure path — it substitutes defaults on a
        // failed read — so there is nothing to match on. Note the substituted default
        // for start is 0, which makes write_start_first() return false and yields the
        // stop-first order. That is the safe fallback: it is the order that was in use
        // before this ordering logic existed.
        let current = get_battery_thresholds().data.unwrap_or(BatteryThresholds {
            start: 0,
            stop: 100,
        });

        let (first_path, first_value, second_path, second_value) =
            if write_start_first(current.start, stop) {
                (start_path, start.to_string(), stop_path, stop.to_string())
            } else {
                (stop_path, stop.to_string(), start_path, start.to_string())
            };

        // Try direct write first
        if fs::write(&first_path, &first_value).is_ok()
            && fs::write(&second_path, &second_value).is_ok()
        {
            return ApiResponse {
                success: true,
                data: Some(format!("Thresholds set: {}%-{}%", start, stop)),
                error: None,
            };
        }

        // Need elevated permissions. Writes stay in the order chosen above.
        let script_content = format!(
            "#!/bin/bash\nset -e\necho {} > {}\necho {} > {}\nexit 0\n",
            first_value, first_path, second_value, second_path
        );

        match crate::privileged::run_script(&script_content).await {
            Ok(output) => {
                if output.status.success() {
                    ApiResponse {
                        success: true,
                        data: Some(format!("Thresholds set: {}%-{}%", start, stop)),
                        error: None,
                    }
                } else {
                    ApiResponse {
                        success: false,
                        data: None,
                        error: Some("Permission denied".to_string()),
                    }
                }
            }
            Err(e) => ApiResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to execute: {}", e)),
            },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PowerConsumptionData {
    pub power_w: f32,
    pub is_charging: bool,
    pub is_ac_online: bool,
    pub status: String,
    pub capacity: u8,
    pub runtime_hours: Option<f32>,
    pub time_to_full_hours: Option<f32>,
}

#[tauri::command]
pub fn get_power_consumption() -> ApiResponse<PowerConsumptionData> {
    #[cfg(target_os = "windows")]
    {
        return windows_battery::get_power_consumption();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let is_ac_online = fs::read_to_string("/sys/class/power_supply/AC/online")
            .map(|s| s.trim() == "1")
            .unwrap_or(false);

        let mut power_w: f32 = 0.0;
        let mut energy_now_wh: f32 = 0.0;
        let mut energy_full_wh: f32 = 0.0;
        let mut status = "Unknown".to_string();
        let mut capacity: u8 = 0;

        for bat_path in [BAT0_PATH, BAT1_PATH] {
            if Path::new(bat_path).exists() {
                let read_f32 = |file: &str| -> f32 {
                    fs::read_to_string(format!("{}/{}", bat_path, file))
                        .ok()
                        .and_then(|s| s.trim().parse::<f32>().ok())
                        .map(|v| v / 1_000_000.0)
                        .unwrap_or(0.0)
                };

                let p = if let Ok(p_str) = fs::read_to_string(format!("{}/power_now", bat_path)) {
                    p_str
                        .trim()
                        .parse::<f32>()
                        .map(|v| v / 1_000_000.0)
                        .unwrap_or(0.0)
                } else {
                    let v = read_f32("voltage_now");
                    let c = read_f32("current_now");
                    v * c
                };
                power_w += p;
                energy_now_wh += read_f32("energy_now");
                energy_full_wh += read_f32("energy_full");

                if let Ok(st) = fs::read_to_string(format!("{}/status", bat_path)) {
                    let st_trimmed = st.trim().to_string();
                    if st_trimmed != "Unknown" && !st_trimmed.is_empty() {
                        status = st_trimmed;
                    }
                }

                if let Ok(cap_str) = fs::read_to_string(format!("{}/capacity", bat_path)) {
                    if let Ok(cap) = cap_str.trim().parse::<u8>() {
                        capacity = cap;
                    }
                }
            }
        }

        let is_charging = status == "Charging";

        let runtime_hours = if !is_charging && !is_ac_online && power_w > 0.05 {
            Some(energy_now_wh / power_w)
        } else {
            None
        };

        let time_to_full_hours = if is_charging && power_w > 0.05 {
            Some(((energy_full_wh - energy_now_wh).max(0.0)) / power_w)
        } else {
            None
        };

        ApiResponse {
            success: true,
            data: Some(PowerConsumptionData {
                power_w,
                is_charging,
                is_ac_online,
                status,
                capacity,
                runtime_hours,
                time_to_full_hours,
            }),
            error: None,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_battery {
    use super::*;
    use serde::Deserialize;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    const UNKNOWN: u8 = 255;
    const BATTERY_FLAG_CHARGING: u8 = 8;
    const BATTERY_FLAG_NO_BATTERY: u8 = 128;
    const STATIC_TTL: Duration = Duration::from_secs(60);

    #[derive(Debug, Deserialize, Clone)]
    struct WinBattery {
        #[serde(default)]
        name: String,
        #[serde(default)]
        manufacturer: String,
        #[serde(default)]
        design_capacity: Option<f64>,
        #[serde(default)]
        full_charge_capacity: Option<f64>,
        #[serde(default)]
        cycle_count: Option<u32>,
        #[serde(default)]
        chemistry: Option<u32>,
    }

    static STATIC_CACHE: Mutex<Option<(Instant, Vec<WinBattery>)>> = Mutex::new(None);

    fn power_status() -> Result<SYSTEM_POWER_STATUS, String> {
        let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        let ok = unsafe { GetSystemPowerStatus(&mut status) };
        if ok == 0 {
            Err("GetSystemPowerStatus failed".to_string())
        } else {
            Ok(status)
        }
    }

    fn battery_status_label(status: &SYSTEM_POWER_STATUS) -> String {
        if status.BatteryFlag & BATTERY_FLAG_NO_BATTERY != 0 || status.BatteryLifePercent == UNKNOWN
        {
            return "No battery".to_string();
        }
        if status.BatteryFlag & BATTERY_FLAG_CHARGING != 0 {
            return "Charging".to_string();
        }
        if status.ACLineStatus == 1 {
            if status.BatteryLifePercent >= 99 {
                return "Full".to_string();
            }
            return "Not charging".to_string();
        }
        "Discharging".to_string()
    }

    fn static_batteries() -> Vec<WinBattery> {
        if let Ok(guard) = STATIC_CACHE.lock() {
            if let Some((at, batteries)) = guard.as_ref() {
                if at.elapsed() < STATIC_TTL {
                    return batteries.clone();
                }
            }
        }

        let script = r#"
Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue |
    Select-Object Name, Manufacturer, DesignCapacity, FullChargeCapacity, CycleCount, Chemistry |
    ConvertTo-Json -Compress
"#;

        let parsed = crate::platform::powershell(script)
            .ok()
            .and_then(|raw| {
                let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
                if value.is_array() {
                    serde_json::from_value::<Vec<WinBattery>>(value).ok()
                } else {
                    serde_json::from_value::<WinBattery>(value)
                        .ok()
                        .map(|b| vec![b])
                }
            })
            .unwrap_or_default();

        if let Ok(mut guard) = STATIC_CACHE.lock() {
            *guard = Some((Instant::now(), parsed.clone()));
        }
        parsed
    }

    fn chemistry_name(code: Option<u32>) -> String {
        match code {
            Some(3) => "Lead Acid",
            Some(4) => "Nickel Cadmium",
            Some(5) => "Nickel Metal Hydride",
            Some(6) => "Lithium-ion",
            Some(7) => "Zinc air",
            Some(8) => "Lithium polymer",
            _ => "Unknown",
        }
        .to_string()
    }

    pub fn get_battery_info() -> ApiResponse<Vec<BatteryInfo>> {
        let status = match power_status() {
            Ok(status) => status,
            Err(e) => {
                return ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e),
                }
            }
        };

        if status.BatteryFlag & BATTERY_FLAG_NO_BATTERY != 0 || status.BatteryLifePercent == UNKNOWN
        {
            return ApiResponse {
                success: false,
                data: None,
                error: Some("No battery found".to_string()),
            };
        }

        let statics = static_batteries();
        let capacity = status.BatteryLifePercent.min(100);
        let batteries = if statics.is_empty() {
            vec![BatteryInfo {
                name: "Battery".to_string(),
                status: battery_status_label(&status),
                capacity,
                health: 100,
                cycles: 0,
                voltage: 0.0,
                current: 0.0,
                power: 0.0,
                energy_now: 0.0,
                energy_full: 0.0,
                energy_design: 0.0,
                technology: "Unknown".to_string(),
                manufacturer: "Unknown".to_string(),
            }]
        } else {
            statics
                .into_iter()
                .enumerate()
                .map(|(index, battery)| {
                    let design_wh = battery.design_capacity.unwrap_or(0.0) / 1000.0;
                    let full_wh = battery.full_charge_capacity.unwrap_or(0.0) / 1000.0;
                    let health = if design_wh > 0.0 && full_wh > 0.0 {
                        ((full_wh / design_wh) * 100.0).clamp(0.0, 100.0) as u8
                    } else {
                        100
                    };
                    let energy_now = if full_wh > 0.0 {
                        (full_wh * capacity as f64 / 100.0) as f32
                    } else {
                        0.0
                    };

                    BatteryInfo {
                        name: if battery.name.is_empty() {
                            format!("Battery {}", index + 1)
                        } else {
                            battery.name
                        },
                        status: battery_status_label(&status),
                        capacity,
                        health,
                        cycles: battery.cycle_count.unwrap_or(0),
                        voltage: 0.0,
                        current: 0.0,
                        power: 0.0,
                        energy_now,
                        energy_full: full_wh as f32,
                        energy_design: design_wh as f32,
                        technology: chemistry_name(battery.chemistry),
                        manufacturer: if battery.manufacturer.is_empty() {
                            "Unknown".to_string()
                        } else {
                            battery.manufacturer
                        },
                    }
                })
                .collect()
        };

        ApiResponse {
            success: true,
            data: Some(batteries),
            error: None,
        }
    }

    pub fn get_power_consumption() -> ApiResponse<PowerConsumptionData> {
        let status = match power_status() {
            Ok(status) => status,
            Err(e) => {
                return ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e),
                }
            }
        };

        let is_ac_online = status.ACLineStatus == 1;
        let is_charging = status.BatteryFlag & BATTERY_FLAG_CHARGING != 0;
        let label = battery_status_label(&status);
        let capacity = if status.BatteryLifePercent == UNKNOWN {
            0
        } else {
            status.BatteryLifePercent.min(100)
        };

        // Windows does not report instantaneous battery power through this API.
        // Leave it at zero rather than inventing a number; the UI shows "not
        // reported" for a zero discharge reading.
        let runtime_hours = if !is_ac_online
            && !is_charging
            && status.BatteryLifeTime != u32::MAX
            && status.BatteryLifeTime > 0
        {
            Some(status.BatteryLifeTime as f32 / 3600.0)
        } else {
            None
        };

        ApiResponse {
            success: true,
            data: Some(PowerConsumptionData {
                power_w: 0.0,
                is_charging,
                is_ac_online,
                status: label,
                capacity,
                runtime_hours,
                time_to_full_hours: None,
            }),
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Model the firmware constraint: a write is rejected if it would leave
    /// start >= stop, even for an instant. Replays the two writes in the chosen
    /// order and reports whether every intermediate state stayed legal.
    fn ordering_is_safe(current: (u8, u8), target: (u8, u8)) -> bool {
        let (cur_start, cur_stop) = current;
        let (new_start, new_stop) = target;

        let states = if write_start_first(cur_start, new_stop) {
            // write start, then stop
            [(new_start, cur_stop), (new_start, new_stop)]
        } else {
            // write stop, then start
            [(cur_start, new_stop), (new_start, new_stop)]
        };

        states.iter().all(|(s, e)| s < e)
    }

    /// The case the original ordering logic missed. With `new_stop < current_start`
    /// this picks stop-first, producing the intermediate state (60, 60) — start
    /// equal to stop, which the firmware rejects with the same "Invalid argument"
    /// the ordering was introduced to prevent.
    #[test]
    fn boundary_new_stop_equals_current_start() {
        assert!(
            write_start_first(60, 60),
            "new_stop == current_start must write start first"
        );
        assert!(ordering_is_safe((60, 80), (40, 60)));
    }

    #[test]
    fn lowering_thresholds_writes_start_first() {
        assert!(write_start_first(60, 50));
        assert!(ordering_is_safe((60, 80), (30, 50)));
    }

    #[test]
    fn raising_thresholds_writes_stop_first() {
        assert!(!write_start_first(40, 90));
        assert!(ordering_is_safe((40, 60), (70, 90)));
    }

    /// Exhaustive sweep over every legal current/target pair. Any ordering rule
    /// that admits an illegal intermediate state fails here.
    #[test]
    fn no_transition_passes_through_an_illegal_state() {
        for cur_start in 0..=100u8 {
            for cur_stop in (cur_start + 1)..=100u8 {
                for new_start in 0..=100u8 {
                    for new_stop in (new_start + 1)..=100u8 {
                        assert!(
                            ordering_is_safe((cur_start, cur_stop), (new_start, new_stop)),
                            "illegal intermediate state going from ({}, {}) to ({}, {})",
                            cur_start,
                            cur_stop,
                            new_start,
                            new_stop
                        );
                    }
                }
            }
        }
    }

    /// A failed sysfs read substitutes start = 0, which must fall back to the
    /// stop-first order that predates this logic rather than doing something novel.
    #[test]
    fn unreadable_current_start_falls_back_to_stop_first() {
        assert!(!write_start_first(0, 80));
    }
}

#[cfg(test)]
mod threshold_path_tests {
    use super::*;

    /// The generic kernel API must be preferred. Both spellings exist on a
    /// ThinkPad and report the same value, but only the generic one exists on
    /// other hardware, so choosing the legacy pair first would silently limit
    /// support to ThinkPads.
    #[test]
    fn prefers_the_generic_kernel_attribute_names() {
        assert_eq!(
            THRESHOLD_ATTRS[0],
            (
                "charge_control_start_threshold",
                "charge_control_end_threshold"
            )
        );
    }

    /// The legacy thinkpad_acpi spelling stays as a fallback for older kernels
    /// that expose only it.
    #[test]
    fn keeps_the_legacy_spelling_as_a_fallback() {
        assert!(THRESHOLD_ATTRS
            .iter()
            .any(|(s, e)| *s == "charge_start_threshold" && *e == "charge_stop_threshold"));
    }

    /// Start and stop must never come from different naming schemes: writing a
    /// generic start and a legacy stop would touch two different sysfs files and
    /// could leave the pair inconsistent.
    #[test]
    fn each_candidate_pair_uses_one_naming_scheme() {
        for (start, stop) in THRESHOLD_ATTRS {
            let start_is_generic = start.starts_with("charge_control_");
            let stop_is_generic = stop.starts_with("charge_control_");
            assert_eq!(
                start_is_generic, stop_is_generic,
                "mixed naming scheme in pair ({}, {})",
                start, stop
            );
        }
    }
}
