use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};

static ACTIVE_FP_PID: AtomicU32 = AtomicU32::new(0);

fn kill_active_fp_process() {
    let old_pid = ACTIVE_FP_PID.swap(0, Ordering::SeqCst);
    if old_pid > 0 {
        let _ = Command::new("kill")
            .args(["-TERM", &old_pid.to_string()])
            .output();
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintEnrollProgress {
    pub stage_number: u32,
    pub total_stages: u32,
    pub percent: u32,
    pub status: String,
    pub message: String,
    pub done: bool,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintVerifyProgress {
    pub status: String,
    pub message: String,
    pub done: bool,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricInstallProgress {
    pub target: String,
    pub percent: u32,
    pub stage: String,
    pub log_line: String,
    pub done: bool,
    pub success: bool,
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
                    // Filter out metadata streams (e.g. index 1); only index 0 devices support video capture
                    let index_file = path.join("index");
                    if let Ok(idx_str) = fs::read_to_string(&index_file) {
                        if idx_str.trim() != "0" {
                            continue;
                        }
                    }

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
            if content.contains("pam_howdy.so")
                || content.contains("howdy/pam.py")
                || content.contains("howdy")
            {
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

pub fn is_howdy_installed() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let user_bin_howdy = format!("{}/.local/bin/howdy", home);
    Path::new("/usr/bin/howdy").exists()
        || Path::new("/usr/local/bin/howdy").exists()
        || (!home.is_empty() && Path::new(&user_bin_howdy).exists())
        || Path::new("/lib/security/howdy/cli.py").exists()
        || Path::new("/usr/lib/security/howdy/cli.py").exists()
        || Command::new("which")
            .arg("howdy")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

pub fn get_howdy_version() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let user_bin_howdy = format!("{}/.local/bin/howdy", home);

    let direct_cmd = if Path::new("/usr/bin/howdy").exists() {
        Command::new("howdy").arg("version").output()
    } else if Path::new("/usr/local/bin/howdy").exists() {
        Command::new("/usr/local/bin/howdy").arg("version").output()
    } else if !home.is_empty() && Path::new(&user_bin_howdy).exists() {
        Command::new(&user_bin_howdy).arg("version").output()
    } else {
        Command::new("/usr/bin/python3")
            .args(["/lib/security/howdy/cli.py", "version"])
            .output()
    };

    direct_cmd
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            let first = text.lines().next().map(|l| l.trim().to_string());
            if first.as_ref().map(|l| !l.contains("root") && !l.is_empty()).unwrap_or(false) {
                first
            } else {
                None
            }
        })
        .or_else(|| {
            Command::new("dpkg")
                .args(["-s", "howdy"])
                .output()
                .ok()
                .and_then(|o| {
                    let text = String::from_utf8_lossy(&o.stdout);
                    for line in text.lines() {
                        if line.starts_with("Version:") {
                            return Some(line.trim_start_matches("Version:").trim().to_string());
                        }
                    }
                    None
                })
        })
}

fn get_current_login_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

fn build_howdy_bash_command(subcommand: &str) -> String {
    let user = get_current_login_user();
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    let xauth = std::env::var("XAUTHORITY").unwrap_or_else(|_| format!("/home/{}/.Xauthority", user));
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());

    let user_arg = if user != "root" && !user.is_empty() {
        format!("-U '{}'", user)
    } else {
        String::new()
    };

    let subcmd_exec = if !subcommand.trim().is_empty() {
        format!("/usr/bin/howdy {} {}", user_arg, subcommand)
    } else {
        String::new()
    };

    format!(
        "\
export USER='{user}'
export SUDO_USER='{user}'
export LOGNAME='{user}'
export DISPLAY='{display}'
export WAYLAND_DISPLAY='{wayland}'
export XAUTHORITY='{xauth}'
export XDG_RUNTIME_DIR='{xdg_runtime}'

if [ ! -x /usr/bin/howdy ] || ! grep -q 'SUDO_USER' /usr/bin/howdy 2>/dev/null; then
  cat << 'EOF' > /usr/bin/howdy
#!/bin/sh
if [ -z \"$SUDO_USER\" ] || [ \"$SUDO_USER\" = \"root\" ]; then
  if [ -n \"$USER\" ] && [ \"$USER\" != \"root\" ]; then
    export SUDO_USER=\"$USER\"
  elif [ -n \"$PKEXEC_UID\" ]; then
    export SUDO_USER=$(id -un \"$PKEXEC_UID\" 2>/dev/null)
  else
    export SUDO_USER=$(logname 2>/dev/null || who | awk '{{print $1}}' | head -n 1)
  fi
fi
exec /usr/bin/python3 /lib/security/howdy/cli.py \"$@\"
EOF
  chmod 755 /usr/bin/howdy 2>/dev/null || true
  ln -sf /usr/bin/howdy /usr/local/bin/howdy 2>/dev/null || true
fi
chmod 755 /lib/security/howdy/cli.py 2>/dev/null || true
mkdir -p /lib/security/howdy/models 2>/dev/null || true
chmod 755 /lib/security/howdy/models 2>/dev/null || true

# Fix pam.py Python 3 compatibility
if [ -f /lib/security/howdy/pam.py ] && grep -q 'import ConfigParser' /lib/security/howdy/pam.py 2>/dev/null; then
  sed -i 's/import ConfigParser/try:\\n    import configparser\\nexcept ImportError:\\n    import ConfigParser as configparser/' /lib/security/howdy/pam.py 2>/dev/null || true
fi

# Ensure suppress_unknown=true and dark_threshold is tolerant in config.ini
if [ -f /lib/security/howdy/config.ini ]; then
  sed -i 's|^suppress_unknown = .*|suppress_unknown = true|' /lib/security/howdy/config.ini 2>/dev/null || true
  sed -i 's|^dark_threshold = .*|dark_threshold = 85|' /lib/security/howdy/config.ini 2>/dev/null || true
fi

# Ensure PAM common-auth includes Howdy
if [ -f /lib/security/howdy/pam.py ] && [ -f /etc/pam.d/common-auth ]; then
  if ! grep -q 'howdy/pam.py' /etc/pam.d/common-auth 2>/dev/null; then
    sed -i '/pam_unix\\.so/i auth\\t[success=end default=ignore]\\tpam_python.so /lib/security/howdy/pam.py' /etc/pam.d/common-auth 2>/dev/null || true
  fi
fi

# Ensure PAM common-auth includes fprintd if present
if [ -f /usr/lib/x86_64-linux-gnu/security/pam_fprintd.so ] || [ -f /lib/x86_64-linux-gnu/security/pam_fprintd.so ]; then
  if [ -f /etc/pam.d/common-auth ] && ! grep -q 'pam_fprintd.so' /etc/pam.d/common-auth 2>/dev/null; then
    sed -i '/pam_unix\\.so/i auth\\t[success=end default=ignore]\\tpam_fprintd.so max-tries=1 timeout=10' /etc/pam.d/common-auth 2>/dev/null || true
  fi
fi

if [ ! -f /lib/security/howdy/dlib-data/shape_predictor_5_face_landmarks.dat ]; then
  if [ -x /lib/security/howdy/dlib-data/install.sh ]; then
    (cd /lib/security/howdy/dlib-data && ./install.sh) 2>/dev/null || true
  fi
fi
/usr/bin/python3 -c 'import dlib' 2>/dev/null || /usr/bin/python3 -m pip install --break-system-packages dlib 2>/dev/null || true
{subcmd_exec}
",
        user = user,
        display = display,
        wayland = wayland,
        xauth = xauth,
        xdg_runtime = xdg_runtime,
        subcmd_exec = subcmd_exec
    )
}

#[tauri::command]
pub fn get_face_recognition_status() -> FaceRecognitionStatus {
    let (cameras, recommended_camera, ir_camera_found) = detect_cameras();

    let howdy_installed = is_howdy_installed();
    let howdy_version = if howdy_installed {
        get_howdy_version()
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
            "/lib/security/howdy/config.ini",
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

        // 1. Try reading the JSON files in /lib/security/howdy/models/ or /etc/howdy/models/
        for models_dir in &["/lib/security/howdy/models", "/etc/howdy/models"] {
            if let Ok(entries) = fs::read_dir(models_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "dat").unwrap_or(false) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            if let Ok(json_models) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                                for jm in json_models {
                                    let id = jm.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default();
                                    let label = jm.get("label").and_then(|v| v.as_str()).unwrap_or("Profile").to_string();
                                    let time_sec = jm.get("time").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let date = if time_sec > 0 {
                                        let naive = chrono::DateTime::from_timestamp(time_sec, 0);
                                        naive.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_else(|| "Enrolled".to_string())
                                    } else {
                                        "Enrolled".to_string()
                                    };
                                    models.push(FaceModel { id, label, date });
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. If no direct files read, try howdy list command
        if models.is_empty() {
            let list_cmd = if Path::new("/usr/bin/howdy").exists() {
                Command::new("howdy").arg("list").output()
            } else if Path::new("/usr/local/bin/howdy").exists() {
                Command::new("/usr/local/bin/howdy").arg("list").output()
            } else if Path::new("/lib/security/howdy/cli.py").exists() {
                Command::new("/usr/bin/python3")
                    .args(["/lib/security/howdy/cli.py", "list"])
                    .output()
            } else {
                Command::new("howdy").arg("list").output()
            };
            if let Ok(output) = list_cmd {
                let text = String::from_utf8_lossy(&output.stdout);
                models = parse_howdy_list_output(&text);
            }
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
pub async fn enroll_fingerprint(
    app: tauri::AppHandle,
    finger: Option<String>,
) -> BiometricActionResponse {
    kill_active_fp_process();

    let target_finger = finger
        .filter(|f| !f.trim().is_empty())
        .unwrap_or_else(|| "right-index-finger".to_string());

    let user = std::env::var("USER").unwrap_or_default();
    let mut cmd = tokio::process::Command::new("fprintd-enroll");
    cmd.args(["-f", &target_finger]);
    if !user.is_empty() {
        cmd.arg(&user);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return BiometricActionResponse {
                success: false,
                message: format!("Failed to start fprintd-enroll: {}", e),
                logs: vec![e.to_string()],
                error: Some(e.to_string()),
            };
        }
    };

    if let Some(pid) = child.id() {
        ACTIVE_FP_PID.store(pid, Ordering::SeqCst);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut logs = Vec::new();
    let mut stage_number = 0u32;
    let mut total_stages = 5u32;
    let mut completed_ok = false;
    let mut err_msg: Option<String> = None;

    let initial_evt = FingerprintEnrollProgress {
        stage_number: 0,
        total_stages,
        percent: 5,
        status: "waiting".to_string(),
        message: format!(
            "Sensor active for {}. Place your finger on the sensor...",
            format_finger_label(&target_finger)
        ),
        done: false,
        success: false,
    };
    let _ = app.emit_to("main", "fingerprint-enroll-progress", &initial_evt);
    let _ = app.emit("fingerprint-enroll-progress", &initial_evt);

    if let Some(out) = stdout {
        let mut reader = BufReader::new(out).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if line_trimmed.is_empty() {
                continue;
            }
            logs.push(line_trimmed.clone());

            let mut evt = FingerprintEnrollProgress {
                stage_number,
                total_stages,
                percent: std::cmp::min(10 + (stage_number * 80) / total_stages, 90),
                status: "waiting".to_string(),
                message: line_trimmed.clone(),
                done: false,
                success: false,
            };

            if line_trimmed.contains("enroll-stage-passed") {
                stage_number += 1;
                if stage_number > total_stages {
                    total_stages = stage_number + 1;
                }
                evt.stage_number = stage_number;
                evt.total_stages = total_stages;
                evt.percent = std::cmp::min(15 + (stage_number * 75) / total_stages, 90);
                evt.status = "stage-passed".to_string();
                evt.message = format!(
                    "Stage {} recorded. Lift and place your finger again...",
                    stage_number
                );
            } else if line_trimmed.contains("enroll-retry-scan") {
                evt.status = "retry-scan".to_string();
                evt.message = "Scan was not clear. Press your finger flat on the sensor.".to_string();
            } else if line_trimmed.contains("enroll-swipe-too-short") {
                evt.status = "swipe-too-short".to_string();
                evt.message = "Touch was too brief. Hold your finger slightly longer.".to_string();
            } else if line_trimmed.contains("enroll-finger-not-centered") {
                evt.status = "not-centered".to_string();
                evt.message = "Finger not centered. Center your finger on the sensor.".to_string();
            } else if line_trimmed.contains("enroll-remove-and-retry") {
                evt.status = "remove-and-retry".to_string();
                evt.message = "Remove your finger and place it again.".to_string();
            } else if line_trimmed.contains("enroll-completed") {
                stage_number = total_stages;
                evt.stage_number = total_stages;
                evt.total_stages = total_stages;
                evt.percent = 100;
                evt.status = "completed".to_string();
                evt.message = format!(
                    "{} enrolled successfully!",
                    format_finger_label(&target_finger)
                );
                evt.done = true;
                evt.success = true;
                completed_ok = true;
            } else if line_trimmed.contains("enroll-unknown-error")
                || line_trimmed.contains("enroll-disconnected")
                || line_trimmed.contains("failed")
            {
                evt.status = "error".to_string();
                evt.message = format!("Enrollment error: {}", line_trimmed);
                evt.done = true;
                evt.success = false;
                err_msg = Some(line_trimmed);
            }

            let _ = app.emit_to("main", "fingerprint-enroll-progress", &evt);
            let _ = app.emit("fingerprint-enroll-progress", &evt);

            if completed_ok {
                break;
            }
        }
    }

    if let Some(err) = stderr {
        let mut err_reader = BufReader::new(err).lines();
        while let Ok(Some(line)) = err_reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if !line_trimmed.is_empty() {
                logs.push(format!("STDERR: {}", line_trimmed));
                if err_msg.is_none() {
                    err_msg = Some(line_trimmed);
                }
            }
        }
    }

    let status_res = child.wait().await;
    ACTIVE_FP_PID.store(0, Ordering::SeqCst);

    let success = completed_ok || status_res.map(|s| s.success()).unwrap_or(false);

    if !success && err_msg.is_none() {
        err_msg = Some("Enrollment process stopped before completing".to_string());
    }

    BiometricActionResponse {
        success,
        message: if success {
            format!("{} enrolled successfully!", format_finger_label(&target_finger))
        } else {
            err_msg.clone().unwrap_or_else(|| "Enrollment failed".to_string())
        },
        logs,
        error: if success { None } else { err_msg },
    }
}

#[tauri::command]
pub fn cancel_fingerprint_action() -> BiometricActionResponse {
    let pid = ACTIVE_FP_PID.swap(0, Ordering::SeqCst);
    if pid > 0 {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
        BiometricActionResponse {
            success: true,
            message: "Biometric action cancelled".to_string(),
            logs: vec![],
            error: None,
        }
    } else {
        BiometricActionResponse {
            success: true,
            message: "No active biometric operation".to_string(),
            logs: vec![],
            error: None,
        }
    }
}

#[tauri::command]
pub async fn verify_fingerprint(
    app: tauri::AppHandle,
    finger: Option<String>,
) -> BiometricActionResponse {
    kill_active_fp_process();

    let user = std::env::var("USER").unwrap_or_default();
    let mut cmd = tokio::process::Command::new("fprintd-verify");
    if let Some(ref f) = finger {
        if !f.trim().is_empty() {
            cmd.args(["-f", f]);
        }
    }
    if !user.is_empty() {
        cmd.arg(&user);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return BiometricActionResponse {
                success: false,
                message: format!("Failed to start fprintd-verify: {}", e),
                logs: vec![e.to_string()],
                error: Some(e.to_string()),
            };
        }
    };

    if let Some(pid) = child.id() {
        ACTIVE_FP_PID.store(pid, Ordering::SeqCst);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut logs = Vec::new();
    let mut verified_ok = false;
    let mut done = false;
    let mut result_msg = String::new();

    let initial_evt = FingerprintVerifyProgress {
        status: "waiting".to_string(),
        message: "Sensor active: touch the fingerprint reader now to verify...".to_string(),
        done: false,
        success: false,
    };
    let _ = app.emit_to("main", "fingerprint-verify-progress", &initial_evt);
    let _ = app.emit("fingerprint-verify-progress", &initial_evt);

    if let Some(out) = stdout {
        let mut reader = BufReader::new(out).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if line_trimmed.is_empty() {
                continue;
            }
            logs.push(line_trimmed.clone());

            let mut evt = FingerprintVerifyProgress {
                status: "waiting".to_string(),
                message: line_trimmed.clone(),
                done: false,
                success: false,
            };

            if line_trimmed.contains("verify-match") {
                evt.status = "match".to_string();
                evt.message = "Fingerprint verified successfully! Match confirmed.".to_string();
                evt.done = true;
                evt.success = true;
                verified_ok = true;
                done = true;
                result_msg = evt.message.clone();
            } else if line_trimmed.contains("verify-no-match") {
                evt.status = "no-match".to_string();
                evt.message = "Fingerprint did not match. Please try again.".to_string();
                evt.done = true;
                evt.success = false;
                done = true;
                result_msg = evt.message.clone();
            } else if line_trimmed.contains("verify-retry-scan") {
                evt.status = "retry-scan".to_string();
                evt.message = "Scan unclear. Center finger on sensor and press firmly.".to_string();
            } else if line_trimmed.contains("verify-swipe-too-short") {
                evt.status = "swipe-too-short".to_string();
                evt.message = "Touch was too brief. Hold finger slightly longer.".to_string();
            } else if line_trimmed.contains("verify-unknown-error") {
                evt.status = "error".to_string();
                evt.message = "Verification error encountered.".to_string();
                evt.done = true;
                evt.success = false;
                done = true;
                result_msg = evt.message.clone();
            }

            let _ = app.emit_to("main", "fingerprint-verify-progress", &evt);
            let _ = app.emit("fingerprint-verify-progress", &evt);

            if done {
                break;
            }
        }
    }

    if let Some(err) = stderr {
        let mut err_reader = BufReader::new(err).lines();
        while let Ok(Some(line)) = err_reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if !line_trimmed.is_empty() {
                logs.push(format!("STDERR: {}", line_trimmed));
                if line_trimmed.contains("NoEnrolledPrints") {
                    result_msg = "No fingerprints are enrolled yet. Please enroll a finger first.".to_string();
                    let evt = FingerprintVerifyProgress {
                        status: "error".to_string(),
                        message: result_msg.clone(),
                        done: true,
                        success: false,
                    };
                    let _ = app.emit_to("main", "fingerprint-verify-progress", &evt);
                    let _ = app.emit("fingerprint-verify-progress", &evt);
                }
            }
        }
    }

    let _ = child.wait().await;
    ACTIVE_FP_PID.store(0, Ordering::SeqCst);

    BiometricActionResponse {
        success: verified_ok,
        message: if result_msg.is_empty() {
            if verified_ok {
                "Fingerprint verified successfully".to_string()
            } else {
                "Verification failed or timed out".to_string()
            }
        } else {
            result_msg
        },
        logs,
        error: if verified_ok { None } else { Some("Verification did not match".to_string()) },
    }
}

#[tauri::command]
pub async fn install_fingerprint_packages(app: tauri::AppHandle) -> BiometricActionResponse {
    let script = "\
export DEBIAN_FRONTEND=noninteractive
echo '=== [1/3] Updating package repositories ==='
apt-get update -y
echo '=== [2/3] Installing fprintd and PAM modules ==='
apt-get install -y fprintd libpam-fprintd
echo '=== [3/3] Enabling fprintd background service ==='
systemctl enable --now fprintd
echo '=== ALL_DONE ==='
";

    let mut cmd = tokio::process::Command::new("pkexec");
    cmd.args(["bash", "-c", script]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return BiometricActionResponse {
                success: false,
                message: format!("Failed to launch installer: {}", e),
                logs: vec![e.to_string()],
                error: Some(e.to_string()),
            };
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut logs = Vec::new();
    let mut current_percent = 5u32;
    let mut current_stage = "Starting installation...".to_string();

    let initial_evt = BiometricInstallProgress {
        target: "fingerprint".to_string(),
        percent: current_percent,
        stage: current_stage.clone(),
        log_line: "Initializing fprintd package installation...".to_string(),
        done: false,
        success: false,
    };
    let _ = app.emit_to("main", "biometric-install-progress", &initial_evt);
    let _ = app.emit("biometric-install-progress", &initial_evt);

    if let Some(out) = stdout {
        let mut reader = BufReader::new(out).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if line_trimmed.is_empty() {
                continue;
            }
            logs.push(line_trimmed.clone());

            if line_trimmed.contains("=== [1/3]") {
                current_stage = "Updating package index...".to_string();
                current_percent = 15;
            } else if line_trimmed.starts_with("Hit:") || line_trimmed.starts_with("Get:") {
                current_percent = std::cmp::min(current_percent + 2, 45);
            } else if line_trimmed.contains("=== [2/3]") {
                current_stage = "Installing fprintd & libpam-fprintd...".to_string();
                current_percent = 50;
            } else if line_trimmed.contains("Unpacking") {
                current_stage = "Unpacking packages...".to_string();
                current_percent = 70;
            } else if line_trimmed.contains("Setting up") {
                current_stage = "Configuring fprintd...".to_string();
                current_percent = 85;
            } else if line_trimmed.contains("=== [3/3]") {
                current_stage = "Enabling fprintd service...".to_string();
                current_percent = 92;
            } else if line_trimmed.contains("=== ALL_DONE ===") {
                current_stage = "Installation completed!".to_string();
                current_percent = 100;
            }

            let evt = BiometricInstallProgress {
                target: "fingerprint".to_string(),
                percent: current_percent,
                stage: current_stage.clone(),
                log_line: line_trimmed,
                done: current_percent >= 100,
                success: current_percent >= 100,
            };
            let _ = app.emit_to("main", "biometric-install-progress", &evt);
            let _ = app.emit("biometric-install-progress", &evt);
        }
    }

    if let Some(err) = stderr {
        let mut err_reader = BufReader::new(err).lines();
        while let Ok(Some(line)) = err_reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if !line_trimmed.is_empty() {
                logs.push(format!("ERR: {}", line_trimmed));
            }
        }
    }

    let status = child.wait().await;
    let is_ok = status.map(|s| s.success()).unwrap_or(false);

    let final_evt = BiometricInstallProgress {
        target: "fingerprint".to_string(),
        percent: if is_ok { 100 } else { current_percent },
        stage: if is_ok {
            "fprintd installed successfully!".to_string()
        } else {
            "Installation failed".to_string()
        },
        log_line: if is_ok {
            "✓ Fingerprint packages and PAM module installed.".to_string()
        } else {
            "✗ Installation encountered an error.".to_string()
        },
        done: true,
        success: is_ok,
    };
    let _ = app.emit_to("main", "biometric-install-progress", &final_evt);
    let _ = app.emit("biometric-install-progress", &final_evt);

    BiometricActionResponse {
        success: is_ok,
        message: if is_ok {
            "fprintd and libpam-fprintd installed successfully".to_string()
        } else {
            "Failed to install fingerprint packages".to_string()
        },
        logs,
        error: if is_ok { None } else { Some("Package installation exited with error".to_string()) },
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

    let cmd = format!(
        "sed -i 's|^device_path = .*|device_path = {cam}|' /etc/howdy/config.ini 2>/dev/null; sed -i 's|^device_path = .*|device_path = {cam}|' /lib/security/howdy/config.ini 2>/dev/null",
        cam = device_path
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
pub async fn install_howdy_packages(
    app: tauri::AppHandle,
    ir_camera: Option<String>,
) -> BiometricActionResponse {
    let camera_target = ir_camera
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| "/dev/video2".to_string());

    let script = format!(
        "\
export DEBIAN_FRONTEND=noninteractive
export HOWDY_NO_PROMPT=1
export PIP_BREAK_SYSTEM_PACKAGES=1
echo '=== [1/5] Updating package index ==='
apt-get update -y
echo '=== [2/5] Installing camera & PAM dependencies ==='
apt-get install -y python3-opencv v4l-utils ffmpeg libpam-python python3-pip python3-setuptools cmake build-essential libdlib-dev bzip2 curl wget || apt-get install -y python3-opencv v4l-utils ffmpeg bzip2 curl wget
echo '=== [3/5] Setting up Howdy package ==='
if [ ! -f /lib/security/howdy/cli.py ]; then
  curl -fsSL -o /tmp/howdy_2.6.1.deb https://github.com/boltgolt/howdy/releases/download/v2.6.1/howdy_2.6.1.deb || wget -O /tmp/howdy_2.6.1.deb https://github.com/boltgolt/howdy/releases/download/v2.6.1/howdy_2.6.1.deb
  dpkg -i /tmp/howdy_2.6.1.deb || apt-get install -f -y
  rm -f /tmp/howdy_2.6.1.deb
fi
echo '=== [4/5] Creating Howdy CLI wrapper and downloading neural models ==='
cat << 'EOF' > /usr/bin/howdy
#!/bin/sh
if [ -z \"$SUDO_USER\" ] || [ \"$SUDO_USER\" = \"root\" ]; then
  if [ -n \"$USER\" ] && [ \"$USER\" != \"root\" ]; then
    export SUDO_USER=\"$USER\"
  elif [ -n \"$PKEXEC_UID\" ]; then
    export SUDO_USER=$(id -un \"$PKEXEC_UID\" 2>/dev/null)
  else
    export SUDO_USER=$(logname 2>/dev/null || who | awk '{{print $1}}' | head -n 1)
  fi
fi
exec /usr/bin/python3 /lib/security/howdy/cli.py \"$@\"
EOF
chmod 755 /usr/bin/howdy /lib/security/howdy/cli.py 2>/dev/null || true
ln -sf /usr/bin/howdy /usr/local/bin/howdy 2>/dev/null || true
if [ ! -f /lib/security/howdy/dlib-data/shape_predictor_5_face_landmarks.dat ]; then
  if [ -x /lib/security/howdy/dlib-data/install.sh ]; then
    (cd /lib/security/howdy/dlib-data && ./install.sh) || true
  fi
fi
/usr/bin/python3 -c 'import dlib' 2>/dev/null || /usr/bin/python3 -m pip install --break-system-packages dlib 2>/dev/null || true
echo '=== [5/5] Configuring IR camera and PAM ==='
if [ -f /lib/security/howdy/config.ini ]; then
  sed -i 's|^device_path = .*|device_path = {camera}|' /lib/security/howdy/config.ini
fi
if [ -f /etc/howdy/config.ini ]; then
  sed -i 's|^device_path = .*|device_path = {camera}|' /etc/howdy/config.ini
fi
pam-auth-update --package 2>/dev/null || true
echo '=== ALL_DONE ==='
",
        camera = camera_target
    );

    let mut cmd = tokio::process::Command::new("pkexec");
    cmd.args(["bash", "-c", &script]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return BiometricActionResponse {
                success: false,
                message: format!("Failed to launch installer: {}", e),
                logs: vec![e.to_string()],
                error: Some(e.to_string()),
            };
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let mut logs = Vec::new();
    let mut current_percent = 5u32;
    let mut current_stage = "Starting Howdy installation...".to_string();

    let initial_evt = BiometricInstallProgress {
        target: "face".to_string(),
        percent: current_percent,
        stage: current_stage.clone(),
        log_line: "Initializing Howdy facial recognition installation...".to_string(),
        done: false,
        success: false,
    };
    let _ = app.emit_to("main", "biometric-install-progress", &initial_evt);
    let _ = app.emit("biometric-install-progress", &initial_evt);

    if let Some(out) = stdout {
        let mut reader = BufReader::new(out).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if line_trimmed.is_empty() {
                continue;
            }
            logs.push(line_trimmed.clone());

            if line_trimmed.contains("=== [1/5]") {
                current_stage = "Updating package repository index...".to_string();
                current_percent = 15;
            } else if line_trimmed.contains("=== [2/5]") {
                current_stage = "Installing camera & PAM dependencies...".to_string();
                current_percent = 35;
            } else if line_trimmed.contains("=== [3/5]") {
                current_stage = "Setting up Howdy package...".to_string();
                current_percent = 55;
            } else if line_trimmed.contains("=== [4/5]") {
                current_stage = "Creating CLI wrapper and downloading neural models...".to_string();
                current_percent = 75;
            } else if line_trimmed.contains("=== [5/5]") {
                current_stage = format!("Configuring IR camera on {}...", camera_target);
                current_percent = 90;
            } else if line_trimmed.contains("=== ALL_DONE ===") {
                current_stage = "Installation completed!".to_string();
                current_percent = 100;
            } else if line_trimmed.starts_with("Hit:") || line_trimmed.starts_with("Get:") {
                current_percent = std::cmp::min(current_percent + 1, 95);
            }

            let evt = BiometricInstallProgress {
                target: "face".to_string(),
                percent: current_percent,
                stage: current_stage.clone(),
                log_line: line_trimmed,
                done: current_percent >= 100,
                success: current_percent >= 100,
            };
            let _ = app.emit_to("main", "biometric-install-progress", &evt);
            let _ = app.emit("biometric-install-progress", &evt);
        }
    }

    if let Some(err) = stderr {
        let mut err_reader = BufReader::new(err).lines();
        while let Ok(Some(line)) = err_reader.next_line().await {
            let line_trimmed = line.trim().to_string();
            if !line_trimmed.is_empty() {
                logs.push(format!("ERR: {}", line_trimmed));
            }
        }
    }

    let status = child.wait().await;
    let is_ok = status.map(|s| s.success()).unwrap_or(false);

    let final_evt = BiometricInstallProgress {
        target: "face".to_string(),
        percent: if is_ok { 100 } else { current_percent },
        stage: if is_ok {
            "Howdy installed successfully!".to_string()
        } else {
            "Installation failed".to_string()
        },
        log_line: if is_ok {
            format!("✓ Howdy installed and configured for {}", camera_target)
        } else {
            "✗ Installation encountered an error.".to_string()
        },
        done: true,
        success: is_ok,
    };
    let _ = app.emit_to("main", "biometric-install-progress", &final_evt);
    let _ = app.emit("biometric-install-progress", &final_evt);

    BiometricActionResponse {
        success: is_ok,
        message: if is_ok {
            format!("Howdy installed and configured for {}", camera_target)
        } else {
            "Failed to install Howdy facial recognition".to_string()
        },
        logs,
        error: if is_ok { None } else { Some("Installation exited with error".to_string()) },
    }
}

#[tauri::command]
pub async fn add_face_model(
    label: String,
    camera_device: Option<String>,
) -> BiometricActionResponse {
    let clean_label = if label.trim().is_empty() {
        "Default".to_string()
    } else {
        label.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "")
    };

    let user = get_current_login_user();

    // Determine target camera: use provided if valid, or auto-detect recommended IR camera
    let cam = camera_device
        .filter(|c| !c.trim().is_empty() && c.starts_with("/dev/video"))
        .unwrap_or_else(|| {
            let (_, rec, _) = detect_cameras();
            rec.unwrap_or_else(|| "/dev/video2".to_string())
        });

    let howdy_add = build_howdy_bash_command("add -y");
    let full_script = format!(
        "\
sed -i 's|^device_path = .*|device_path = {cam}|' /etc/howdy/config.ini 2>/dev/null || true
sed -i 's|^device_path = .*|device_path = {cam}|' /lib/security/howdy/config.ini 2>/dev/null || true
{howdy_add}
STATUS=$?
if [ $STATUS -eq 0 ]; then
  /usr/bin/python3 -c \"
import json, os
for p in ['/lib/security/howdy/models/{user}.dat', '/etc/howdy/models/{user}.dat']:
    if os.path.exists(p):
        try:
            with open(p, 'r') as f: data = json.load(f)
            if data and isinstance(data, list) and len(data) > 0:
                data[-1]['label'] = '{clean_label}'
                with open(p, 'w') as f: json.dump(data, f)
                os.chmod(p, 0o644)
        except Exception:
            pass
\"
  chmod 755 /lib/security/howdy/models 2>/dev/null || true
  chmod 644 /lib/security/howdy/models/*.dat 2>/dev/null || true

  # Ensure PAM is active for login and sudo
  if [ -x /usr/sbin/pam-auth-update ]; then
    DEBIAN_FRONTEND=noninteractive /usr/sbin/pam-auth-update --package --enable howdy 2>/dev/null || true
  fi
  if [ -f /etc/pam.d/common-auth ] && ! grep -q -E 'howdy|pam_python.*pam\\.py' /etc/pam.d/common-auth 2>/dev/null; then
    sed -i '/pam_unix\\.so/i auth\\t[success=end default=ignore]\\tpam_python.so /lib/security/howdy/pam.py' /etc/pam.d/common-auth 2>/dev/null || true
  fi
fi
exit $STATUS
",
        cam = cam,
        howdy_add = howdy_add,
        user = user,
        clean_label = clean_label
    );

    let output = Command::new("pkexec")
        .args(["bash", "-c", &full_script])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let success = out.status.success()
                || stdout.contains("Added a new model")
                || stdout.contains("Face model")
                || stdout.contains("Model #");

            BiometricActionResponse {
                success,
                message: if success {
                    format!("Face model '{}' added successfully", clean_label)
                } else {
                    let err_msg = if !stderr.trim().is_empty() {
                        stderr.trim().to_string()
                    } else if !stdout.trim().is_empty() {
                        stdout
                            .lines()
                            .last()
                            .unwrap_or("Failed to add face model")
                            .trim()
                            .to_string()
                    } else {
                        "Failed to add face model".to_string()
                    };
                    format!("Failed to add face model: {}", err_msg)
                },
                logs: vec![stdout, stderr.clone()],
                error: if success { None } else { Some(stderr) },
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: format!("Failed to run howdy add: {}", e),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn configure_biometric_pam() -> BiometricActionResponse {
    let script = "\
# 1. Fix pam.py Python 3 compatibility
if [ -f /lib/security/howdy/pam.py ] && grep -q 'import ConfigParser' /lib/security/howdy/pam.py 2>/dev/null; then
  sed -i 's/import ConfigParser/try:\\n    import configparser\\nexcept ImportError:\\n    import ConfigParser as configparser/' /lib/security/howdy/pam.py 2>/dev/null || true
fi

# 2. Ensure suppress_unknown=true and dark_threshold is tolerant in config.ini
for cfg in /lib/security/howdy/config.ini /etc/howdy/config.ini; do
  if [ -f \"$cfg\" ]; then
    sed -i 's|^suppress_unknown = .*|suppress_unknown = true|' \"$cfg\" 2>/dev/null || true
    sed -i 's|^dark_threshold = .*|dark_threshold = 85|' \"$cfg\" 2>/dev/null || true
  fi
done

# 3. Update PAM via pam-auth-update if available
if [ -x /usr/sbin/pam-auth-update ]; then
  DEBIAN_FRONTEND=noninteractive /usr/sbin/pam-auth-update --package --enable howdy 2>/dev/null || true
  if [ -f /usr/lib/x86_64-linux-gnu/security/pam_fprintd.so ] || [ -f /lib/x86_64-linux-gnu/security/pam_fprintd.so ]; then
    DEBIAN_FRONTEND=noninteractive /usr/sbin/pam-auth-update --package --enable fprintd 2>/dev/null || true
  fi
fi

# 4. Fallback check for Debian /etc/pam.d/common-auth
if [ -f /etc/pam.d/common-auth ]; then
  if ! grep -q -E 'howdy|pam_python.*pam\\.py' /etc/pam.d/common-auth 2>/dev/null; then
    sed -i '/pam_unix\\.so/i auth\\t[success=end default=ignore]\\tpam_python.so /lib/security/howdy/pam.py' /etc/pam.d/common-auth 2>/dev/null || true
  fi
  if ([ -f /usr/lib/x86_64-linux-gnu/security/pam_fprintd.so ] || [ -f /lib/x86_64-linux-gnu/security/pam_fprintd.so ]) && ! grep -q 'pam_fprintd\\.so' /etc/pam.d/common-auth 2>/dev/null; then
    sed -i '/pam_unix\\.so/i auth\\t[success=end default=ignore]\\tpam_fprintd.so max-tries=1 timeout=10' /etc/pam.d/common-auth 2>/dev/null || true
  fi
fi

# 5. Check for Arch/Fedora system-auth
if [ -f /etc/pam.d/system-auth ]; then
  if [ -f /lib64/security/pam_howdy.so ] || [ -f /usr/lib/security/pam_howdy.so ]; then
    if ! grep -q 'pam_howdy\\.so' /etc/pam.d/system-auth 2>/dev/null; then
      sed -i '/pam_unix\\.so/i auth\\tsufficient\\tpam_howdy.so' /etc/pam.d/system-auth 2>/dev/null || true
    fi
  fi
fi
";

    let output = Command::new("pkexec")
        .args(["bash", "-c", script])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let (fprint_pam, howdy_pam) = check_pam_status();
            if out.status.success() || howdy_pam || fprint_pam {
                BiometricActionResponse {
                    success: true,
                    message: "Biometric PAM authentication configured successfully".to_string(),
                    logs: vec![stdout],
                    error: None,
                }
            } else {
                BiometricActionResponse {
                    success: false,
                    message: "Failed to configure PAM for biometrics".to_string(),
                    logs: vec![stderr.clone()],
                    error: Some(stderr),
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: format!("Failed to run PAM configuration: {}", e),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn test_face_model() -> BiometricActionResponse {
    let user = get_current_login_user();

    // Check if models exist first before touching camera
    let has_models = ["/lib/security/howdy/models", "/etc/howdy/models"]
        .iter()
        .any(|dir| {
            let user_dat = Path::new(dir).join(format!("{}.dat", user));
            if user_dat.exists() {
                if let Ok(metadata) = fs::metadata(&user_dat) {
                    if metadata.len() > 10 {
                        return true;
                    }
                }
            }
            false
        });

    if !has_models {
        return BiometricActionResponse {
            success: false,
            message: "No face profile enrolled yet. Please click 'Add Face Profile' first.".to_string(),
            logs: vec!["No face model found for current user".to_string()],
            error: Some("No face profile enrolled".to_string()),
        };
    }

    let script = format!(
        "\
pkill -9 -f 'howdy.*compare.py' 2>/dev/null || true
pkill -9 -f 'howdy.*cli.py.*test' 2>/dev/null || true
timeout 8 /usr/bin/python3 /lib/security/howdy/compare.py '{user}'
"
    );

    let output = Command::new("pkexec")
        .args(["bash", "-c", &script])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let code = out.status.code().unwrap_or(-1);

            match code {
                0 => BiometricActionResponse {
                    success: true,
                    message: "Face recognized successfully!".to_string(),
                    logs: vec![stdout],
                    error: None,
                },
                10 => BiometricActionResponse {
                    success: false,
                    message: "No face profile found for this user.".to_string(),
                    logs: vec![stdout, stderr.clone()],
                    error: Some("No face model configured".to_string()),
                },
                11 => BiometricActionResponse {
                    success: false,
                    message: "Face recognition timed out. Face not recognized.".to_string(),
                    logs: vec![stdout, stderr.clone()],
                    error: Some("Face not recognized within timeout".to_string()),
                },
                13 => BiometricActionResponse {
                    success: false,
                    message: "Face recognition failed: scene was too dark or IR sensor did not receive enough light.".to_string(),
                    logs: vec![stdout, stderr.clone()],
                    error: Some("Scene too dark".to_string()),
                },
                124 => BiometricActionResponse {
                    success: false,
                    message: "Recognition test timed out after 8 seconds.".to_string(),
                    logs: vec![stdout, stderr.clone()],
                    error: Some("Operation timed out".to_string()),
                },
                _ => {
                    let err_msg = if !stderr.trim().is_empty() {
                        stderr.trim().to_string()
                    } else if !stdout.trim().is_empty() {
                        stdout.trim().to_string()
                    } else {
                        format!("Recognition test exited with code {}", code)
                    };
                    BiometricActionResponse {
                        success: false,
                        message: format!("Face recognition test: {}", err_msg),
                        logs: vec![stdout, stderr.clone()],
                        error: Some(err_msg),
                    }
                }
            }
        }
        Err(e) => BiometricActionResponse {
            success: false,
            message: format!("Failed to run face recognition test: {}", e),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn remove_face_model(id: String) -> BiometricActionResponse {
    let clean_id = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect::<String>();
    let script = build_howdy_bash_command(&format!("remove '{}' -y", clean_id));
    let output = Command::new("pkexec")
        .args(["bash", "-c", &script])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                BiometricActionResponse {
                    success: true,
                    message: format!("Face model #{} removed", clean_id),
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
            message: format!("Failed to run howdy remove: {}", e),
            logs: vec![e.to_string()],
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn clear_face_models() -> BiometricActionResponse {
    let user = get_current_login_user();
    let howdy_clear = build_howdy_bash_command("clear -y");
    let full_script = format!(
        "\
{howdy_clear}
rm -f /lib/security/howdy/models/{user}.dat /etc/howdy/models/{user}.dat 2>/dev/null || true
",
        howdy_clear = howdy_clear,
        user = user
    );

    let output = Command::new("pkexec")
        .args(["bash", "-c", &full_script])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let lower_out = stdout.to_lowercase();
            let lower_err = stderr.to_lowercase();

            let success = out.status.success()
                || lower_out.contains("no models")
                || lower_out.contains("cleared")
                || lower_err.contains("no models")
                || lower_err.contains("cleared");

            if success {
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
            message: format!("Failed to run howdy clear: {}", e),
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

    #[test]
    fn biometric_progress_payloads_serialize_cleanly() {
        let enroll = FingerprintEnrollProgress {
            stage_number: 3,
            total_stages: 5,
            percent: 60,
            status: "stage-passed".to_string(),
            message: "Stage 3 passed".to_string(),
            done: false,
            success: false,
        };
        let enroll_json = serde_json::to_string(&enroll).unwrap();
        assert!(enroll_json.contains("\"stage-passed\""));
        assert!(enroll_json.contains("\"percent\":60"));

        let install = BiometricInstallProgress {
            target: "fingerprint".to_string(),
            percent: 85,
            stage: "Configuring fprintd...".to_string(),
            log_line: "Setting up fprintd...".to_string(),
            done: false,
            success: false,
        };
        let install_json = serde_json::to_string(&install).unwrap();
        assert!(install_json.contains("\"fingerprint\""));
        assert!(install_json.contains("\"percent\":85"));
    }

    #[test]
    fn build_howdy_bash_command_exports_sudo_user() {
        let cmd = build_howdy_bash_command("test");
        assert!(cmd.contains("export SUDO_USER="));
        assert!(cmd.contains("export DISPLAY="));
        assert!(cmd.contains("/usr/bin/howdy"));
    }
}
