//! Small platform helpers shared by the Windows backends.
//!
//! The Linux modules talk to `/proc` and `/sys` directly and do not need any of
//! this. Everything here exists so the Windows code can launch a built-in
//! utility without flashing a console window and so capability detection can
//! ask "is this binary present?" without assuming a POSIX shell.

use std::process::Command;

/// Build a command that will not open a console window on Windows.
///
/// `Command::new` on a GUI subsystem process would otherwise flash a black
/// console for every PowerShell/CIM query.
pub fn command(program: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mut cmd = Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new(program)
    }
}

/// Run a PowerShell snippet and return trimmed stdout.
///
/// Only used for one-shot queries (system information, WMI thermal zones,
/// Defender status/actions). Hot paths use the native Windows APIs directly.
#[cfg(target_os = "windows")]
pub fn powershell(script: &str) -> Result<String, String> {
    let output = command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|e| format!("Failed to launch PowerShell: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

#[cfg(not(target_os = "windows"))]
pub fn powershell(_script: &str) -> Result<String, String> {
    Err("PowerShell is only available on Windows.".to_string())
}

/// Whether a program is on this system's PATH.
pub fn command_exists(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        // `where.exe` is a built-in and avoids depending on cmd.exe parsing.
        command("where.exe")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {}", name))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Human-readable platform name for the capability report.
pub fn os_family() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        "unknown"
    }
}

/// Escape a value for inclusion in a PowerShell single-quoted string.
///
/// A single quote is escaped by doubling it. Newlines are legal inside a
/// single-quoted PowerShell string, so this is sufficient to prevent a path
/// from terminating the string and injecting another command.
pub fn ps_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_quoting_doubles_single_quotes() {
        assert_eq!(ps_single_quote("C:\\Users\\o'brien"), "C:\\Users\\o''brien");
        assert_eq!(
            ps_single_quote("x'; Remove-Item C:\\ -Recurse; '"),
            "x''; Remove-Item C:\\ -Recurse; ''"
        );
    }

    #[test]
    fn os_family_is_known() {
        assert!(matches!(os_family(), "linux" | "windows" | "unknown"));
    }
}
