//! Windows sensor readings for the Home and Fan views.
//!
//! Windows has no `/proc/acpi/ibm/fan`. Fan RPM and fan control are therefore
//! unavailable, and this module says so by returning an empty fan map rather
//! than inventing readings.
//!
//! Temperatures are best-effort from the ACPI thermal zones exposed through
//! WMI. Not every firmware populates them, and the values are zone
//! temperatures, not per-core temperatures. The query is cached because the
//! Home view polls and a PowerShell process every two seconds would cost more
//! than the reading is worth.

use crate::fan_control::{ApiResponse, SensorData};
use crate::platform::powershell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5);
const MIN_CELSIUS: f64 = -20.0;
const MAX_CELSIUS: f64 = 125.0;

static CACHE: Mutex<Option<(Instant, HashMap<String, String>)>> = Mutex::new(None);

pub async fn get_sensor_data() -> ApiResponse<SensorData> {
    let temps = tokio::task::spawn_blocking(read_temperatures)
        .await
        .unwrap_or_default();

    ApiResponse {
        success: true,
        data: Some(SensorData {
            temps,
            // No fan interface on Windows.
            fans: HashMap::new(),
        }),
        error: None,
    }
}

fn read_temperatures() -> HashMap<String, String> {
    if let Ok(guard) = CACHE.lock() {
        if let Some((at, temps)) = guard.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return temps.clone();
            }
        }
    }

    let script = r#"
$zones = @(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue)
$out = @{}
$i = 0
foreach ($z in $zones) {
    $i++
    $celsius = ($z.CurrentTemperature / 10.0) - 273.15
    $name = if ($z.InstanceName) { [string]$z.InstanceName } else { "Thermal zone $i" }
    $out[$name] = [string]([math]::Round($celsius, 1))
}
$out | ConvertTo-Json -Compress
"#;

    let parsed = powershell(script)
        .ok()
        .and_then(|raw| serde_json::from_str::<HashMap<String, String>>(&raw).ok())
        .unwrap_or_default();

    let temps = parse_temperatures(parsed);

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), temps.clone()));
    }

    temps
}

fn parse_temperatures(parsed: HashMap<String, String>) -> HashMap<String, String> {
    let mut temps = HashMap::new();
    for (name, value) in parsed {
        let Ok(celsius) = value.parse::<f64>() else {
            continue;
        };
        // Some firmware reports 0 or an uninitialised sentinel. Showing
        // "0.0°C" would look like a real reading.
        if !(MIN_CELSIUS..=MAX_CELSIUS).contains(&celsius) {
            continue;
        }
        temps.insert(name, format!("{:.1}°C", celsius));
    }
    temps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_plausible_readings_and_drops_sentinels() {
        let parsed = HashMap::from([
            ("CPU".to_string(), "54.3".to_string()),
            ("Uninitialised".to_string(), "0".to_string()),
            ("Bogus".to_string(), "9999".to_string()),
            ("Not a number".to_string(), "n/a".to_string()),
        ]);

        let temps = parse_temperatures(parsed);
        assert_eq!(temps.get("CPU").map(String::as_str), Some("54.3°C"));
        assert_eq!(temps.len(), 1);
    }
}
