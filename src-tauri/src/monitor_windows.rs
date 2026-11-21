//! Native Windows system monitoring.
//!
//! Deliberately not PowerShell. The Monitor view polls every two seconds; a
//! PowerShell process per poll would cost more CPU than the thing it measures.
//! Everything here is a direct `kernel32`/`iphlpapi`/`ntdll` call.
//!
//! Where Windows has no equivalent of a Linux figure, the value is marked
//! rather than invented:
//!
//! * load average is a decaying average of busy CPUs, because Windows has no
//!   load-average API. The Monitor page labels it as an approximation.
//! * per-process CPU is the lifetime average, matching what `ps aux` reports on
//!   Linux rather than a fresh instantaneous sample.
//! * battery power draw is not exposed by `GetSystemPowerStatus`, so the
//!   Battery view gets zero and says "not reported" instead of a guess.

use crate::monitor::{
    ApiResponse, CoreStats, CpuStats, DiskStats, LoadAverage, MemoryStats, NetworkStats,
    ProcessInfo, SystemMonitor,
};
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Wdk::System::SystemInformation::{
    NtQuerySystemInformation, SystemProcessorPerformanceInformation,
};
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Power::{
    CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION,
};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    GetActiveProcessorCount, GetProcessTimes, GetSystemTimes, OpenProcess, ALL_PROCESSOR_GROUPS,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_FIXED, SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
};

const SAMPLE_WINDOW: Duration = Duration::from_millis(200);
const WINDOWS_TO_UNIX_EPOCH_100NS: u64 = 116_444_736_000_000_000;

pub async fn get_system_monitor() -> ApiResponse<SystemMonitor> {
    match tokio::task::spawn_blocking(collect_system_stats).await {
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

fn collect_system_stats() -> Result<SystemMonitor, String> {
    Ok(SystemMonitor {
        cpu: get_cpu_stats()?,
        memory: get_memory_stats()?,
        disk: get_disk_stats()?,
        network: get_network_stats()?,
        processes: get_top_processes(),
    })
}

#[derive(Clone, Copy, Default)]
struct GlobalTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

fn filetime_to_u64(value: &FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}

fn global_times() -> Result<GlobalTimes, String> {
    let mut idle = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut kernel = idle;
    let mut user = idle;

    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    if ok == 0 {
        return Err("GetSystemTimes failed".to_string());
    }

    Ok(GlobalTimes {
        idle: filetime_to_u64(&idle),
        // Kernel time includes idle time. The caller subtracts it.
        kernel: filetime_to_u64(&kernel),
        user: filetime_to_u64(&user),
    })
}

#[derive(Clone, Copy, Default)]
struct CoreTimes {
    idle: i64,
    kernel: i64,
    user: i64,
}

fn core_count() -> usize {
    let count = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } as usize;
    count.max(1)
}

/// One `NtQuerySystemInformation` call returns idle/kernel/user counters for
/// every logical processor. `KernelTime` includes `IdleTime`.
fn core_times(count: usize) -> Vec<CoreTimes> {
    let mut buffer = vec![SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION::default(); count];
    let bytes =
        (buffer.len() * std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>()) as u32;
    let mut returned = 0u32;

    let status = unsafe {
        NtQuerySystemInformation(
            SystemProcessorPerformanceInformation,
            buffer.as_mut_ptr() as *mut c_void,
            bytes,
            &mut returned,
        )
    };

    if status < 0 {
        return Vec::new();
    }

    buffer
        .into_iter()
        .map(|info| CoreTimes {
            idle: info.IdleTime,
            kernel: info.KernelTime,
            user: info.UserTime,
        })
        .collect()
}

fn usage_percent(idle_delta: u64, total_delta: u64) -> f64 {
    if total_delta == 0 {
        return 0.0;
    }
    let active = total_delta.saturating_sub(idle_delta);
    (active as f64 / total_delta as f64) * 100.0
}

fn per_core_usage(before: &[CoreTimes], after: &[CoreTimes]) -> Vec<(usize, f64)> {
    before
        .iter()
        .zip(after.iter())
        .enumerate()
        .map(|(id, (before, after))| {
            let idle = after.idle.saturating_sub(before.idle) as u64;
            let kernel = after.kernel.saturating_sub(before.kernel) as u64;
            let user = after.user.saturating_sub(before.user) as u64;
            // Kernel includes idle, so total is kernel + user and idle is
            // already part of that total.
            (id, usage_percent(idle, kernel + user))
        })
        .collect()
}

fn core_frequencies(count: usize) -> Vec<u64> {
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
        return vec![0; count];
    }

    buffer
        .into_iter()
        .map(|info| info.CurrentMhz as u64)
        .collect()
}

/// Windows has no load average. This keeps a decaying average of busy CPUs so
/// the shared Monitor view has three meaningful numbers instead of zeros. The
/// page labels it as an approximation, and `get_capabilities` says the same.
static LOAD_TRACKER: Mutex<Option<(Instant, f64, f64, f64)>> = Mutex::new(None);

fn update_load(instantaneous: f64) -> LoadAverage {
    let now = Instant::now();
    let mut guard = match LOAD_TRACKER.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let (one, five, fifteen) = match guard.as_ref() {
        Some((at, one, five, fifteen)) => {
            let dt = now.duration_since(*at).as_secs_f64();
            let decay = |value: f64, tau: f64| {
                if dt <= 0.0 {
                    value
                } else {
                    let alpha = 1.0 - (-dt / tau).exp();
                    value + alpha * (instantaneous - value)
                }
            };
            (
                decay(*one, 60.0),
                decay(*five, 300.0),
                decay(*fifteen, 900.0),
            )
        }
        None => (instantaneous, instantaneous, instantaneous),
    };

    *guard = Some((now, one, five, fifteen));

    LoadAverage {
        one_min: one,
        five_min: five,
        fifteen_min: fifteen,
    }
}

fn get_cpu_stats() -> Result<CpuStats, String> {
    let count = core_count();

    let before = global_times()?;
    let cores_before = core_times(count);
    std::thread::sleep(SAMPLE_WINDOW);
    let after = global_times()?;
    let cores_after = core_times(count);

    let idle_delta = after.idle.saturating_sub(before.idle);
    let kernel_delta = after.kernel.saturating_sub(before.kernel);
    let user_delta = after.user.saturating_sub(before.user);
    let total_usage = usage_percent(idle_delta, kernel_delta + user_delta);

    let frequencies = core_frequencies(count);
    let cores = if cores_before.len() == cores_after.len() && !cores_before.is_empty() {
        per_core_usage(&cores_before, &cores_after)
            .into_iter()
            .map(|(id, usage)| CoreStats {
                core_id: id,
                usage_percent: usage,
                frequency: frequencies.get(id).copied().unwrap_or(0),
            })
            .collect()
    } else {
        Vec::new()
    };

    let instantaneous_load = (total_usage / 100.0) * count as f64;

    Ok(CpuStats {
        usage_percent: total_usage,
        cores,
        load_avg: update_load(instantaneous_load),
    })
}

fn get_memory_stats() -> Result<MemoryStats, String> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };

    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return Err("GlobalMemoryStatusEx failed".to_string());
    }

    let total = status.ullTotalPhys;
    let available = status.ullAvailPhys;
    let used = total.saturating_sub(available);
    let usage_percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    // The pagefile is the closest Windows equivalent to swap. Commit charge
    // minus physical usage is the part actually backed by the pagefile.
    let swap_total = status.ullTotalPageFile.saturating_sub(status.ullTotalPhys);
    let physical_used = total.saturating_sub(available);
    let commit_used = status
        .ullTotalPageFile
        .saturating_sub(status.ullAvailPageFile);
    let swap_used = commit_used.saturating_sub(physical_used);

    Ok(MemoryStats {
        total,
        used,
        available,
        usage_percent,
        swap_total,
        swap_used,
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

fn drive_root(index: u32) -> String {
    format!("{}:\\", (b'A' + index as u8) as char)
}

fn get_disk_stats() -> Result<Vec<DiskStats>, String> {
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err("GetLogicalDrives failed".to_string());
    }

    let mut disks = Vec::new();

    for index in 0..26u32 {
        if mask & (1 << index) == 0 {
            continue;
        }

        let root = drive_root(index);
        let root_wide = wide(&root);

        if unsafe { GetDriveTypeW(root_wide.as_ptr()) } != DRIVE_FIXED {
            continue;
        }

        let mut free_to_caller = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                root_wide.as_ptr(),
                &mut free_to_caller,
                &mut total,
                &mut total_free,
            )
        };
        if ok == 0 || total == 0 {
            continue;
        }

        let mut volume_name = [0u16; 256];
        let mut filesystem = [0u16; 64];
        let volume_ok = unsafe {
            GetVolumeInformationW(
                root_wide.as_ptr(),
                volume_name.as_mut_ptr(),
                volume_name.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            )
        };

        let label = if volume_ok != 0 {
            wide_to_string(&volume_name)
        } else {
            String::new()
        };
        let filesystem = if volume_ok != 0 {
            wide_to_string(&filesystem)
        } else {
            "Unknown".to_string()
        };
        let filesystem = if filesystem.is_empty() {
            "Unknown".to_string()
        } else {
            filesystem
        };

        let used = total.saturating_sub(total_free);
        let usage_percent = (used as f64 / total as f64) * 100.0;

        disks.push(DiskStats {
            device: if label.is_empty() {
                root.trim_end_matches('\\').to_string()
            } else {
                format!("{} ({})", label, root.trim_end_matches('\\'))
            },
            mount_point: root,
            total,
            used,
            available: free_to_caller,
            usage_percent,
            filesystem,
        });
    }

    Ok(disks)
}

fn get_network_stats() -> Result<Vec<NetworkStats>, String> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let result = unsafe { GetIfTable2(&mut table) };
    if result != 0 || table.is_null() {
        return Err(format!("GetIfTable2 failed with code {}", result));
    }

    // `Table` is a one-element flexible array; NumEntries gives the real
    // length. Reading past the first element needs the pointer form.
    let rows: &[MIB_IF_ROW2] = unsafe {
        std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
    };

    let mut interfaces = Vec::new();
    for row in rows {
        if row.Type == IF_TYPE_SOFTWARE_LOOPBACK {
            continue;
        }

        let alias = wide_to_string(&row.Alias);
        let description = wide_to_string(&row.Description);
        let interface = if alias.is_empty() { description } else { alias };
        if interface.is_empty() {
            continue;
        }

        interfaces.push(NetworkStats {
            interface,
            rx_bytes: row.InOctets,
            tx_bytes: row.OutOctets,
            rx_packets: row.InUcastPkts.saturating_add(row.InNUcastPkts),
            tx_packets: row.OutUcastPkts.saturating_add(row.OutNUcastPkts),
        });
    }

    unsafe { FreeMibTable(table as *const c_void) };

    Ok(interfaces)
}

fn process_name(entry: &PROCESSENTRY32W) -> String {
    // PROCESSENTRY32W uses CHAR (i8) for its name field.
    let bytes: Vec<u8> = entry
        .szExeFile
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).to_string()
}

fn process_metrics(pid: u32) -> Option<(f64, f64)> {
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }

    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;

    let times_ok =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let memory_ok = unsafe {
        K32GetProcessMemoryInfo(
            handle,
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };

    unsafe { CloseHandle(handle) };

    let memory_mb = if memory_ok != 0 {
        counters.WorkingSetSize as f64 / 1024.0 / 1024.0
    } else {
        0.0
    };

    let cpu_percent = if times_ok != 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
            / 100
            + WINDOWS_TO_UNIX_EPOCH_100NS;
        let elapsed = now.saturating_sub(filetime_to_u64(&creation));
        let cpu = filetime_to_u64(&kernel).saturating_add(filetime_to_u64(&user));

        if elapsed > 0 {
            (cpu as f64 / elapsed as f64) * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    Some((cpu_percent, memory_mb))
}

fn get_top_processes() -> Vec<ProcessInfo> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return Vec::new();
    }

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut processes = Vec::new();

    if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
        loop {
            let name = process_name(&entry);
            if !name.is_empty() && entry.th32ProcessID != 0 {
                let (cpu_percent, memory_mb) =
                    process_metrics(entry.th32ProcessID).unwrap_or((0.0, 0.0));

                processes.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    name,
                    cpu_percent,
                    memory_mb,
                    status: "Running".to_string(),
                });
            }

            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }

    unsafe { CloseHandle(snapshot) };

    processes.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    processes.truncate(10);
    processes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_subtracts_idle_from_kernel_inclusive_total() {
        // 100 ticks total, 40 idle: 60% busy.
        assert!((usage_percent(40, 100) - 60.0).abs() < 0.001);
    }

    #[test]
    fn zero_total_does_not_divide_by_zero() {
        assert_eq!(usage_percent(0, 0), 0.0);
    }

    #[test]
    fn wide_strings_round_trip() {
        let encoded = wide("C:\\");
        assert_eq!(wide_to_string(&encoded), "C:\\");
    }
}
