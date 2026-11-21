//! Windows performance backend.
//!
//! Windows has no cpufreq governor or turbo-boost switch. What it does have is
//! a documented power-plan interface (`powercfg`), which maps cleanly onto the
//! three profile buttons the UI already shows. Governor and turbo controls
//! return explicit "not available on Windows" errors instead of pretending to
//! succeed.

use crate::performance::{ApiResponse, CpuInfo, PowerProfile};
use crate::platform::command;
use std::ffi::c_void;
use windows_sys::Win32::System::Power::{
    CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION,
};
use windows_sys::Win32::System::Threading::{GetActiveProcessorCount, ALL_PROCESSOR_GROUPS};

const POWER_SAVER_GUID: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";
const BALANCED_GUID: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const PERFORMANCE_GUID: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

const NO_GOVERNOR: &str = "Windows manages CPU frequency through the selected power plan. Linux-style governors are not exposed.";

fn powercfg(args: &[&str]) -> Result<String, String> {
    let output = command("powercfg.exe")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run powercfg: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn extract_guid(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text[start..].find('}')? + start;
    Some(text[start + 1..end].trim().to_lowercase())
}

fn active_guid() -> Result<String, String> {
    let output = powercfg(&["/getactivescheme"])?;
    extract_guid(&output).ok_or_else(|| "Could not read the active power plan.".to_string())
}

fn known_plans() -> [(&'static str, &'static str); 3] {
    [
        ("power-saver", POWER_SAVER_GUID),
        ("balanced", BALANCED_GUID),
        ("performance", PERFORMANCE_GUID),
    ]
}

fn slug_for_guid(guid: &str) -> Option<&'static str> {
    known_plans()
        .into_iter()
        .find(|(_, plan_guid)| *plan_guid == guid)
        .map(|(slug, _)| slug)
}

/// The friendly name from `powercfg /getactivescheme`, which is localised. The
/// output looks like `Power Scheme GUID: {guid}  (Balanced)`.
fn active_name(output: &str) -> Option<String> {
    let open = output.find('(')?;
    let close = output[open..].find(')')? + open;
    let name = output[open + 1..close].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn cpu_frequencies() -> (u32, u32) {
    let count = (unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } as usize).max(1);
    let mut buffer = vec![PROCESSOR_POWER_INFORMATION::default(); count];
    let bytes = (buffer.len() * std::mem::size_of::<PROCESSOR_POWER_INFORMATION>()) as u32;

    let status = unsafe {
        CallNtPowerInformation(
            ProcessorInformation,
            std::ptr::null(),
            0,
            buffer.as_mut_ptr() as *mut c_void,
            bytes,
        )
    };

    if status < 0 {
        return (0, 0);
    }

    let current = buffer.iter().map(|p| p.CurrentMhz).max().unwrap_or(0);
    let max = buffer
        .iter()
        .map(|p| if p.MhzLimit > 0 { p.MhzLimit } else { p.MaxMhz })
        .max()
        .unwrap_or(0);

    (current, max)
}

pub fn get_cpu_info() -> ApiResponse<CpuInfo> {
    let (current, max) = cpu_frequencies();

    ApiResponse {
        success: true,
        data: Some(CpuInfo {
            governor: "windows".to_string(),
            min_freq: 0,
            max_freq: max,
            current_freq: current,
            available_governors: Vec::new(),
        }),
        error: None,
    }
}

pub async fn set_cpu_governor(governor: String) -> ApiResponse<String> {
    let _ = governor;
    ApiResponse {
        success: false,
        data: None,
        error: Some(NO_GOVERNOR.to_string()),
    }
}

pub fn get_power_profile() -> ApiResponse<PowerProfile> {
    let active = match active_guid() {
        Ok(guid) => guid,
        Err(e) => {
            return ApiResponse {
                success: false,
                data: None,
                error: Some(e),
            }
        }
    };

    let list = match powercfg(&["/list"]) {
        Ok(list) => list.to_lowercase(),
        Err(e) => {
            return ApiResponse {
                success: false,
                data: None,
                error: Some(e),
            }
        }
    };

    let available: Vec<String> = known_plans()
        .into_iter()
        .filter(|(_, guid)| list.contains(guid))
        .map(|(slug, _)| slug.to_string())
        .collect();

    if available.is_empty() {
        return ApiResponse {
            success: false,
            data: None,
            error: Some("powercfg reported no usable power plans.".to_string()),
        };
    }

    // An OEM or custom plan is still a real plan. Report its localised name
    // rather than mislabelling it as one of the three standard ones.
    let current = match slug_for_guid(&active) {
        Some(slug) => slug.to_string(),
        None => active_name(&powercfg(&["/getactivescheme"]).unwrap_or_default())
            .unwrap_or_else(|| "custom".to_string()),
    };

    ApiResponse {
        success: true,
        data: Some(PowerProfile { current, available }),
        error: None,
    }
}

pub async fn set_power_profile(profile: String) -> ApiResponse<String> {
    let Some((slug, guid)) = known_plans().into_iter().find(|(slug, _)| *slug == profile) else {
        return ApiResponse {
            success: false,
            data: None,
            error: Some(format!(
                "Unknown power profile '{}'. Windows supports power-saver, balanced and performance.",
                profile
            )),
        };
    };

    let list = match powercfg(&["/list"]) {
        Ok(list) => list.to_lowercase(),
        Err(e) => {
            return ApiResponse {
                success: false,
                data: None,
                error: Some(e),
            }
        }
    };

    if !list.contains(guid) {
        return ApiResponse {
            success: false,
            data: None,
            error: Some(format!(
                "The '{}' power plan is not available on this machine.",
                slug
            )),
        };
    }

    if let Err(e) = powercfg(&["/setactive", guid]) {
        return ApiResponse {
            success: false,
            data: None,
            error: Some(format!("Failed to switch power plan: {}", e)),
        };
    }

    // Read back rather than trusting the exit code: a policy can accept the
    // command and keep the old plan.
    match active_guid() {
        Ok(active) if active == guid => ApiResponse {
            success: true,
            data: Some(format!("Power profile set to: {}", slug)),
            error: None,
        },
        Ok(active) => ApiResponse {
            success: false,
            data: None,
            error: Some(format!(
                "Windows kept power plan {} instead of '{}'. Group policy may prevent the change.",
                active, slug
            )),
        },
        Err(e) => ApiResponse {
            success: false,
            data: None,
            error: Some(e),
        },
    }
}

pub fn get_turbo_boost_status() -> ApiResponse<bool> {
    ApiResponse {
        success: false,
        data: None,
        error: Some(
            "Turbo boost is managed by the Windows power engine and firmware. ThinkUtils does not override it."
                .to_string(),
        ),
    }
}

pub async fn set_turbo_boost(enabled: bool) -> ApiResponse<String> {
    let _ = enabled;
    ApiResponse {
        success: false,
        data: None,
        error: Some(
            "Turbo boost is managed by the Windows power engine and firmware. ThinkUtils does not override it."
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_guid_from_powercfg_output() {
        let output = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)";
        assert_eq!(
            extract_guid(output).as_deref(),
            Some("381b4222-f694-41f0-9685-ff5bb260df2e")
        );
    }

    #[test]
    fn maps_known_guids_to_slugs() {
        assert_eq!(slug_for_guid(BALANCED_GUID), Some("balanced"));
        assert_eq!(slug_for_guid(POWER_SAVER_GUID), Some("power-saver"));
        assert_eq!(slug_for_guid(PERFORMANCE_GUID), Some("performance"));
        assert_eq!(slug_for_guid("00000000-0000-0000-0000-000000000000"), None);
    }

    #[test]
    fn reads_localised_plan_name() {
        let output = "Power Scheme GUID: {00000000-0000-0000-0000-000000000000}  (Ausbalanciert)";
        assert_eq!(active_name(output).as_deref(), Some("Ausbalanciert"));
    }

    #[test]
    fn guid_extraction_is_case_insensitive() {
        let output = "GUID: {A1841308-3541-4FAB-BC81-F71556F20B4A}";
        assert_eq!(
            extract_guid(output).as_deref(),
            Some("a1841308-3541-4fab-bc81-f71556f20b4a")
        );
    }
}
