use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintDeviceInfo {
    pub found: bool,
    pub name: String,
    pub vendor_id: String,
    pub product_id: String,
    pub bus: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolledFinger {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintStatus {
    pub device: FingerprintDeviceInfo,
    pub fprintd_installed: bool,
    pub service_active: bool,
    pub enrolled_fingers: Vec<EnrolledFinger>,
    pub pam_auth_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraDevice {
    pub path: String,
    pub name: String,
    pub is_infrared: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceModel {
    pub id: String,
    pub label: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceRecognitionStatus {
    pub ir_camera_found: bool,
    pub recommended_camera: Option<String>,
    pub cameras: Vec<CameraDevice>,
    pub howdy_installed: bool,
    pub howdy_version: Option<String>,
    pub configured_device: Option<String>,
    pub models: Vec<FaceModel>,
    pub ir_emitter_installed: bool,
    pub pam_auth_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricOverview {
    pub fingerprint: FingerprintStatus,
    pub face_recognition: FaceRecognitionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricActionResponse {
    pub success: bool,
    pub message: String,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

/// Friendly human-readable names for standard fprintd finger codes
pub fn format_finger_label(id: &str) -> String {
    match id {
        "right-thumb" => "Right Thumb",
        "right-index-finger" => "Right Index Finger",
        "right-middle-finger" => "Right Middle Finger",
        "right-ring-finger" => "Right Ring Finger",
        "right-little-finger" => "Right Little Finger",
        "left-thumb" => "Left Thumb",
        "left-index-finger" => "Left Index Finger",
        "left-middle-finger" => "Left Middle Finger",
        "left-ring-finger" => "Left Ring Finger",
        "left-little-finger" => "Left Little Finger",
        other => other,
    }
    .to_string()
}

/// Detects fingerprint reader hardware on the system (Goodix, Synaptics, Elan, Validity, etc.)
pub fn detect_fingerprint_device() -> FingerprintDeviceInfo {
    let mut best_device = FingerprintDeviceInfo {
        found: false,
        name: String::new(),
        vendor_id: String::new(),
        product_id: String::new(),
        bus: None,
    };

    // Known biometric fingerprint sensor USB vendor IDs
    // 27c6 = Goodix (popular on ThinkPad T14/X1/P14)
    // 06cb = Synaptics
    // 04f3 = Elan Microelectronics
    // 138a = Validity Sensors
    // 2808 = FocalTech
    // 10a5 = Fingerprint Cards AB
    let known_vendors = ["27c6", "06cb", "04f3", "138a", "2808", "10a5"];

    let usb_dir = Path::new("/sys/bus/usb/devices");
    if usb_dir.exists() {
        if let Ok(entries) = fs::read_dir(usb_dir) {
            for entry in entries.flatten() {
                let dev_path = entry.path();
                let v_file = dev_path.join("idVendor");
                let p_file = dev_path.join("idProduct");

                if v_file.exists() && p_file.exists() {
                    let vid = fs::read_to_string(&v_file)
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase();
                    let pid = fs::read_to_string(&p_file)
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase();

                    let prod_file = dev_path.join("product");
                    let product = fs::read_to_string(&prod_file)
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                    let is_known_vendor = known_vendors.contains(&vid.as_str());
                    let has_fp_keyword = product.to_lowercase().contains("fingerprint")
                        || product.to_lowercase().contains("goodix");

                    if is_known_vendor || has_fp_keyword {
                        let name = if !product.is_empty() {
                            format!("{} ({}:{})", product, vid, pid)
                        } else {
                            format!("Fingerprint Sensor ({}:{})", vid, pid)
                        };

                        best_device = FingerprintDeviceInfo {
                            found: true,
                            name,
                            vendor_id: vid,
                            product_id: pid,
                            bus: entry.file_name().to_str().map(|s| s.to_string()),
                        };
                        break;
                    }
                }
            }
        }
    }

    // Fallback: Check lsusb output if sysfs was empty
    if !best_device.found {
        if let Ok(output) = Command::new("lsusb").output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let lower = line.to_lowercase();
                    if lower.contains("goodix")
                        || lower.contains("fingerprint")
                        || lower.contains("synaptics")
                    {
                        let name = line.split("ID ").nth(1).unwrap_or(line).trim().to_string();
                        best_device = FingerprintDeviceInfo {
                            found: true,
                            name,
                            vendor_id: String::new(),
                            product_id: String::new(),
                            bus: None,
                        };
                        break;
                    }
                }
            }
        }
    }

    best_device
}

/// Detects all video devices, particularly infrared cameras used for Windows Hello
pub fn detect_cameras() -> (Vec<CameraDevice>, Option<String>, bool) {
    let mut cameras = Vec::new();
    let mut recommended_camera = None;
    let mut ir_found = false;

    let v4l_dir = Path::new("/sys/class/video4linux");
    if v4l_dir.exists() {
        if let Ok(entries) = fs::read_dir(v4l_dir) {
            let mut entries_vec: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            entries_vec.sort();

            for path in entries_vec {
                if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                    let name_file = path.join("name");
                    let name = fs::read_to_string(&name_file)
                        .unwrap_or_else(|_| fname.to_string())
                        .trim()
                        .to_string();

                    let lower_name = name.to_lowercase();
                    let is_ir = lower_name.contains("integrated i")
                        || lower_name.contains("infrared")
                        || lower_name.contains(" ir")
                        || lower_name.contains("-ir");

                    let dev_path = format!("/dev/{}", fname);

                    if is_ir {
                        ir_found = true;
                        if recommended_camera.is_none() {
                            recommended_camera = Some(dev_path.clone());
                        }
                    }

                    cameras.push(CameraDevice {
                        path: dev_path,
                        name,
                        is_infrared: is_ir,
                    });
                }
            }
        }
    }

    (cameras, recommended_camera, ir_found)
}

/// Parse enrolled fingers list from `fprintd-list` output
pub fn parse_fprintd_list_output(output: &str) -> Vec<EnrolledFinger> {
    let mut fingers = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- #") || trimmed.starts_with("#") {
            if let Some(finger_id) = trimmed.split(':').nth(1) {
                let id = finger_id.trim().to_string();
                if !id.is_empty() {
                    let label = format_finger_label(&id);
                    fingers.push(EnrolledFinger { id, label });
                }
            }
        }
    }
    fingers
}

/// Parse enrolled face models from `howdy list` output
pub fn parse_howdy_list_output(output: &str) -> Vec<FaceModel> {
    let mut models = Vec::new();
    let mut past_header = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Header line typically contains ID, Date, Label
        if trimmed.starts_with("ID") || trimmed.contains("Label") {
            past_header = true;
            continue;
        }

        if past_header {
            // E.g.: "0  2026-10-15 12:30:00  Normal"
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                let id = parts[0].to_string();
                let date = format!("{} {}", parts[1], parts.get(2).unwrap_or(&""));
                let label = if parts.len() > 3 {
                    parts[3..].join(" ")
                } else {
                    parts.get(2).unwrap_or(&"Default").to_string()
                };

                models.push(FaceModel { id, label, date });
            }
        }
    }
    models
}

/// Checks if PAM includes biometric modules
pub fn check_pam_status() -> (bool, bool) {
    let mut fprintd_pam = false;
    let mut howdy_pam = false;

    let pam_files = [
        "/etc/pam.d/common-auth",
        "/etc/pam.d/sudo",
        "/etc/pam.d/gdm-fingerprint",
        "/etc/pam.d/gdm-password",
    ];

    for file in &pam_files {
        if let Ok(content) = fs::read_to_string(file) {
            if content.contains("pam_fprintd.so") {
                fprintd_pam = true;
            }
            if content.contains("pam_howdy.so") {
                howdy_pam = true;
            }
        }
    }

    (fprintd_pam, howdy_pam)
}

#[tauri::command]
pub fn get_fingerprint_status() -> FingerprintStatus {
    let device = detect_fingerprint_device();
    let fprintd_installed = Command::new("which")
        .arg("fprintd-list")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let service_active = if fprintd_installed {
        Command::new("systemctl")
            .args(["is-active", "fprintd"])
            .output()
            .map(|o| {
                let text = String::from_utf8_lossy(&o.stdout);
                text.trim() == "active" || text.trim() == "inactive" // service is loaded
            })
            .unwrap_or(false)
    } else {
        false
    };

    let mut enrolled_fingers = Vec::new();
    if fprintd_installed {
        let user = std::env::var("USER").unwrap_or_default();
        if let Ok(output) = Command::new("fprintd-list").arg(&user).output() {
            let text = String::from_utf8_lossy(&output.stdout);
            enrolled_fingers = parse_fprintd_list_output(&text);
        }
    }

    let (pam_fprintd, _) = check_pam_status();

    FingerprintStatus {
        device,
        fprintd_installed,
        service_active,
        enrolled_fingers,
        pam_auth_enabled: pam_fprintd,
    }
}

#[tauri::command]
pub fn get_face_recognition_status() -> FaceRecognitionStatus {
    let (cameras, recommended_camera, ir_camera_found) = detect_cameras();

    let howdy_installed = Command::new("which")
        .arg("howdy")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let howdy_version = if howdy_installed {
        Command::new("howdy")
            .arg("version")
            .output()
            .ok()
            .and_then(|o| {
                let text = String::from_utf8_lossy(&o.stdout);
                text.lines().next().map(|l| l.trim().to_string())
            })
    } else {
        None
    };

    let mut configured_device = None;
    let mut models = Vec::new();

    if howdy_installed {
        // Read config.ini to find configured camera device
        let config_paths = [
            "/etc/howdy/config.ini",
            "/usr/local/etc/howdy/config.ini",
            "/usr/lib/security/howdy/config.ini",
        ];
        for cp in &config_paths {
            if let Ok(content) = fs::read_to_string(cp) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("device_path") {
                        if let Some(val) = trimmed.split('=').nth(1) {
                            configured_device = Some(val.trim().to_string());
                            break;
                        }
                    }
                }
                if configured_device.is_some() {
                    break;
                }
            }
        }

        // Read enrolled face models
        if let Ok(output) = Command::new("howdy").arg("list").output() {
            let text = String::from_utf8_lossy(&output.stdout);
            models = parse_howdy_list_output(&text);
        }
    }

    let ir_emitter_installed = Command::new("which")
        .arg("linux-enable-ir-emitter")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let (_, pam_howdy) = check_pam_status();

    FaceRecognitionStatus {
        ir_camera_found,
        recommended_camera,
        cameras,
        howdy_installed,
        howdy_version,
        configured_device,
        models,
        ir_emitter_installed,
        pam_auth_enabled: pam_howdy,
    }
}

#[tauri::command]
pub fn get_biometric_overview() -> BiometricOverview {
    BiometricOverview {
        fingerprint: get_fingerprint_status(),
        face_recognition: get_face_recognition_status(),
    }
}

#[tauri::command]
pub fn delete_fingerprint(finger: Option<String>) -> BiometricActionResponse {
    let user = std::env::var("USER").unwrap_or_default();
    let mut cmd = Command::new("fprintd-delete");

    if let Some(ref f) = finger {
        if !f.is_empty() {
            cmd.args(["-f", f, &user]);
        } else {
            cmd.arg(&user);
        }
    } else {
        cmd.arg(&user);
    }

    match cmd.output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                BiometricActionResponse {
                    success: true,
                    message: "Fingerprint deleted successfully".to_string(),
                    logs: vec![stdout],
                    error: None,
                }
            } else {
                BiometricActionResponse {
                    success: false,
                    message: "Failed to delete fingerprint".to_string(),
                    logs: vec![stderr.clone()],
                    error: Some(stderr),
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: "Failed to execute fprintd-delete".to_string(),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn install_fingerprint_packages() -> BiometricActionResponse {
    let mut logs = Vec::new();
    logs.push("Starting fprintd & libpam-fprintd installation via apt...".to_string());

    let output = Command::new("pkexec")
        .args([
            "apt-get",
            "install",
            "-y",
            "fprintd",
            "libpam-fprintd",
        ])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            for line in stdout.lines() {
                if !line.trim().is_empty() {
                    logs.push(line.to_string());
                }
            }

            if out.status.success() {
                logs.push("✓ Fingerprint packages installed successfully!".to_string());
                BiometricActionResponse {
                    success: true,
                    message: "fprintd installed successfully".to_string(),
                    logs,
                    error: None,
                }
            } else {
                logs.push(format!("✗ Error: {}", stderr));
                BiometricActionResponse {
                    success: false,
                    message: "Failed to install fingerprint packages".to_string(),
                    logs,
                    error: Some(stderr.to_string()),
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: "Failed to launch package manager".to_string(),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn configure_howdy_camera(device_path: String) -> BiometricActionResponse {
    if device_path.is_empty() || !device_path.starts_with("/dev/video") {
        return BiometricActionResponse {
            success: false,
            message: "Invalid video device path".to_string(),
            logs: vec![],
            error: Some("Device path must start with /dev/video".to_string()),
        };
    }

    let config_file = "/etc/howdy/config.ini";
    let cmd = format!(
        "sed -i 's|^device_path = .*|device_path = {}|' {}",
        device_path, config_file
    );

    let output = Command::new("pkexec").args(["bash", "-c", &cmd]).output();

    match output {
        Ok(out) => {
            if out.status.success() {
                BiometricActionResponse {
                    success: true,
                    message: format!("Configured Howdy camera to {}", device_path),
                    logs: vec![format!("device_path set to {}", device_path)],
                    error: None,
                }
            } else {
                let err = String::from_utf8_lossy(&out.stderr).to_string();
                BiometricActionResponse {
                    success: false,
                    message: "Failed to update Howdy configuration".to_string(),
                    logs: vec![err.clone()],
                    error: Some(err),
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: "Execution error".to_string(),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn remove_face_model(id: String) -> BiometricActionResponse {
    let output = Command::new("pkexec")
        .args(["howdy", "remove", &id, "-y"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                BiometricActionResponse {
                    success: true,
                    message: format!("Face model #{} removed", id),
                    logs: vec![stdout],
                    error: None,
                }
            } else {
                BiometricActionResponse {
                    success: false,
                    message: "Failed to remove face model".to_string(),
                    logs: vec![stderr.clone()],
                    error: Some(stderr),
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: "Failed to run howdy remove".to_string(),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn clear_face_models() -> BiometricActionResponse {
    let output = Command::new("pkexec")
        .args(["howdy", "clear", "-y"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                BiometricActionResponse {
                    success: true,
                    message: "All face models cleared".to_string(),
                    logs: vec![stdout],
                    error: None,
                }
            } else {
                BiometricActionResponse {
                    success: false,
                    message: "Failed to clear face models".to_string(),
                    logs: vec![stderr.clone()],
                    error: Some(stderr),
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: "Failed to run howdy clear".to_string(),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_finger_labels_correctly() {
        assert_eq!(format_finger_label("right-index-finger"), "Right Index Finger");
        assert_eq!(format_finger_label("left-thumb"), "Left Thumb");
        assert_eq!(format_finger_label("right-middle-finger"), "Right Middle Finger");
        assert_eq!(format_finger_label("custom-finger"), "custom-finger");
    }

    #[test]
    fn parse_fprintd_list_handles_typical_output() {
        let sample = "found 1 devices\n\
                      Using device /net/reactivated/Fprint/Device/0\n\
                      Fingerprints for user tysontran on Goodix (device 0):\n\
                       - #0: right-index-finger\n\
                       - #1: left-index-finger\n";
        let parsed = parse_fprintd_list_output(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "right-index-finger");
        assert_eq!(parsed[0].label, "Right Index Finger");
        assert_eq!(parsed[1].id, "left-index-finger");
        assert_eq!(parsed[1].label, "Left Index Finger");
    }

    #[test]
    fn parse_howdy_list_handles_tabular_output() {
        let sample = "ID  Date                 Label\n\
                      0   2026-10-15 10:00:00   Normal\n\
                      1   2026-10-15 11:30:00   With Glasses\n";
        let parsed = parse_howdy_list_output(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "0");
        assert_eq!(parsed[0].label, "Normal");
        assert_eq!(parsed[1].id, "1");
        assert_eq!(parsed[1].label, "With Glasses");
    }
}
