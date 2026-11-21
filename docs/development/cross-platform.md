# Cross-Platform Architecture & Compatibility

ThinkUtils keeps one application, one UI and one settings store across Linux and
Windows. Only the code that talks to hardware is platform-specific. The rule is
simple: **the UI never offers a control unless the backend has reported that the
feature is available on this machine**.

## Architecture

```
src/js/                         shared frontend
  capabilities.js               caches backend capability report, gates UI
  views/registry.js             single list of views and their setup/teardown
  views/*.js                    platform-neutral view logic
src-tauri/src/
  capabilities.rs               feature detection (status + human-readable why)
  platform.rs                   command/PowerShell helpers, platform detection
  fan_control.rs / fan_curve.rs Linux thinkpad_acpi backend (cfg-gated commands)
  fan_control_windows.rs        ACPI thermal-zone temperatures (read only)
  battery.rs                    Linux sysfs + windows_battery (GetSystemPowerStatus)
  performance.rs / _windows.rs  cpufreq/ppd/TLP vs powercfg + native CPU frequency
  monitor.rs / _linux.rs / _windows.rs
  security.rs / _linux.rs / _windows.rs
  system_info.rs                DMI/procfs vs CIM/WMI
```

The backend exposes one extra command, `get_capabilities`, which returns:

```json
{
  "platform": "windows",
  "platform_name": "Windows",
  "is_thinkpad": false,
  "features": [
    {
      "id": "fan",
      "title": "Fan Control",
      "status": "monitoring_only",
      "detail": "Windows does not expose the ThinkPad ACPI fan interface...",
      "backend": "acpi-wmi"
    }
  ]
}
```

Statuses are `supported`, `partial`, `monitoring_only`, `admin_required` and
`unsupported`. Only `unsupported` blocks navigation; the other states keep the
page available but hide write controls and display an explanation. If the
capability command itself fails, the frontend fails open to the historical
behaviour (show the control, report the backend error after a click).

Markup opts into gating with `data-capability`, `data-capability-controls` and
`data-capability-notice` attributes. See `src/js/capabilities.js`.

## Compatibility matrix

| Feature | Linux | Windows | Notes |
| :--- | :--- | :--- | :--- |
| Home dashboard | Yes | Yes | Fan tile reads "Unavailable" on Windows. |
| System monitor | Yes | Yes | Native APIs on Windows. Load average is an exponential moving average of busy CPUs, **not** a kernel 1/5/15-min value; the UI labels it "Load (approx.)". Per-process CPU is a lifetime average (same semantics as Linux `ps aux`). |
| System information | Yes | Yes | CIM/WMI on Windows. |
| Battery status | Yes | Yes | Native `GetSystemPowerStatus` + cached `Win32_Battery`. Instantaneous power draw is not reported by Windows and is shown as 0 / "not reported". |
| Charge thresholds | Yes (sysfs, if exposed) | **No** | Lenovo Vantage or the OEM driver owns this; there is no documented Windows API. |
| Fan control / curves | Yes (`thinkpad_acpi`) | **No** | No Windows equivalent of `/proc/acpi/ibm/fan`. Temperature readings are best-effort from `MSAcpi_ThermalZoneTemperature`. |
| Temperature sensors | Yes (`lm-sensors`) | Partial | ACPI thermal zones only; not every firmware populates them. No per-core or fan RPM. |
| CPU frequency display | Yes | Yes | `CallNtPowerInformation(ProcessorInformation)`. |
| CPU governor | Yes (cpufreq) | **No** | Windows has no sysfs-style governor; use a power plan. |
| Turbo boost toggle | Yes (`intel_pstate`/`cpufreq`) | **No** | Managed by the Windows power engine and firmware. |
| Power profile | Yes (`powerprofilesctl`/TLP) | Yes | `powercfg /list`, `/getactivescheme`, `/setactive`, with read-back verification. Standard plans map to the three existing buttons; OEM plans are shown by localised name. |
| Antivirus | ClamAV | Microsoft Defender | Defender status, signature update, quick scan and custom scan via the documented `Defender` PowerShell module. Scanned-file count is not available from `Start-MpScan` and is displayed as "Not reported". Scans may need administrator approval. |
| Fingerprint / biometrics | fprintd | **No** | Windows Hello enrollment is owned by Windows Settings. ThinkUtils does not modify biometric credentials. |
| GNOME top-bar indicator | Yes (GNOME only) | **No** | GNOME Shell extension. |
| AI integration (MCP) | Yes | **No** (this release) | Its tools expose fan, charge-threshold and cpufreq writes with no Windows backend. `start_mcp_server` returns an explicit error. |
| Settings import/export/reset | Yes | Yes | Values for unsupported hardware features are preserved in the JSON but are not applied. |
| pkexec / polkit permission setup | Yes | N/A | Not invoked on Windows. |

## Implemented Windows features

- Native monitoring: CPU usage/per-core, memory/swap, fixed drives, network
  interfaces and process list, collected from `windows-sys` APIs rather than by
  polling PowerShell every two seconds.
- Battery state through `GetSystemPowerStatus` plus a five-minute cache of the
  `Win32_Battery` CIM row.
- Power-plan switching through `powercfg`, including verification that the
  requested plan is actually active after the command succeeds.
- Microsoft Defender status/update/scan through one-shot PowerShell commands.
- Best-effort ACPI thermal-zone temperatures, cached for five seconds so the
  Home and Fan views do not spawn a PowerShell process on every tick.
- Capability report and UI gating so every unsupported feature is explained
  before the user clicks it.

Microsoft Defender is the only antivirus backend on Windows. ThinkUtils does
not install a second engine: the "Install ClamAV" control is hidden and replaced
with a Windows Security explanation.

## Windows prerequisites

- Windows 10 or 11 with WebView2 (the installer can download the bootstrapper;
  see `src-tauri/tauri.conf.json`).
- Microsoft Defender for the Security page.
- No third-party utilities are required. PowerShell 5.1+ is used only for
  one-shot WMI/CIM queries and Defender actions.
- Some actions (Defender scans, signature updates) may require elevation. The
  app runs unprivileged and reports the underlying error instead of silently
  retrying with elevation.

## Linux dependencies (unchanged)

The Linux backend is unchanged. Its optional runtime dependencies are:

- `thinkpad_acpi` with `fan_control=1` for fan control/curves.
- `lm-sensors` (`sensors`) for temperature readings.
- `pkexec`/polkit plus the installed fan helper for privileged writes.
- `power-profiles-daemon` (`powerprofilesctl`) or TLP (`tlp-stat`) for power
  profiles.
- `fprintd` and a supported fingerprint reader for biometrics.
- ClamAV (`clamscan`/`freshclam`) for the antivirus page.
- GNOME Shell for the top-bar indicator.

None of these are required on Windows, and no Windows code path reads a Linux
path or runs a Linux command.

## Building and installing

```bash
# From the repository root
npm run tauri dev     # development, hot reload
npm run tauri build   # production bundles for the current host

# Rust-only checks
cd src-tauri
cargo check
cargo test --all-targets
```

Cross-compiling a Windows binary from Linux is possible with the GNU target:

```bash
rustup target add x86_64-pc-windows-gnu
cd src-tauri && cargo check --target x86_64-pc-windows-gnu
```

A full cross-link needs an lld-based mingw toolchain; some distro `mingw-w64`
packages use GNU `ld`, which rejects the `--dynamicbase` linker flag emitted by
Tauri. Building the final `.exe`/installer is therefore recommended on Windows
itself or in CI with an lld-capable toolchain.

`npm run tauri build` on Windows produces the NSIS installer and MSI (bundle
targets are `all`). The Windows installer uses `perMachine` install mode.

## Verification performed

| Check | Result |
| :--- | :--- |
| `cargo test --all-targets` (Linux) | Pass — 118 unit tests plus integration tests |
| `cargo check` (Linux) | Pass |
| `cargo check --target x86_64-pc-windows-gnu` | Pass |
| `npm run validate` (ESLint, HTMLHint, Stylelint, Prettier) | Pass |
| `npm run docs:build` | Pass |
| `cargo build --target x86_64-pc-windows-gnu` (full link) | Blocked by this Linux host's cross-linker: `x86_64-w64-mingw32-gcc` invokes GNU `ld`, which does not understand Tauri's `--dynamicbase` flag. A Windows host (or an lld-based mingw toolchain) is required for the final link. |

**No physical Windows machine was available in this environment.** A successful
`cargo check` proves only that the Windows code type-checks and that the
platform branches are structurally correct. Runtime behaviour — especially
Defender cmdlets, WMI classes, `powercfg` output on localized systems, and
firmware thermal-zone availability — still needs validation on real Windows 10
and 11 hardware.

## Known issues / limitations

- ACPI thermal zones may be absent or report placeholder values; implausible
  readings are discarded and the UI shows "No data".
- `Start-MpScan` does not return a scanned-file count.
- Windows has no load-average API; the displayed value is an approximation.
- Windows does not expose instantaneous battery power draw through
  `GetSystemPowerStatus`.
- Defender cmdlets may be blocked by organizational policy; the underlying
  PowerShell error is surfaced to the user.
- The GNOME extension, MCP server, fan control, charge thresholds, governor and
  turbo controls are intentionally unavailable on Windows.
- Linux support is unchanged and was verified by the existing test suite, but
  physical hardware regression testing is still recommended after a release.
