// Settings & Backup View
import { showStatus } from '../utils.js';
import { applyAllSettings } from '../settingsManager.js';

const invoke = (...args) => window.__TAURI__?.core?.invoke(...args);

let statusTimeout = null;

export function setupSettingsHandlers() {
  const btnExport = document.getElementById('btn-export-settings');
  const btnImport = document.getElementById('btn-import-settings');
  const btnReset = document.getElementById('btn-reset-settings');
  const fileInput = document.getElementById('input-import-settings');

  if (btnExport) {
    btnExport.addEventListener('click', handleExportSettings);
  }

  if (btnImport && fileInput) {
    btnImport.addEventListener('click', () => {
      fileInput.click();
    });

    fileInput.addEventListener('change', handleImportFileSelected);
  }

  if (btnReset) {
    btnReset.addEventListener('click', handleResetSettings);
  }
}

/**
 * Load settings view data and display current configurations
 */
export async function loadSettingsView() {
  try {
    const settings = await invoke('load_app_settings');
    updateSettingsDisplay(settings);
  } catch (error) {
    console.error('[Settings View] Failed to load settings:', error);
  }
}

async function handleExportSettings() {
  try {
    showSettingsBanner('Preparing settings export...', 'info');
    const jsonString = await invoke('export_settings');

    // Trigger browser/webview file download
    const blob = new Blob([jsonString], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    const dateStr = new Date().toISOString().slice(0, 10);
    a.href = url;
    a.download = `thinkutils-settings-${dateStr}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    // Refresh display to show the synchronized exported settings
    const activeSettings = await invoke('load_app_settings');
    updateSettingsDisplay(activeSettings);

    showSettingsBanner('✓ Settings exported successfully to JSON file', 'success');
    showStatus('✓ Settings exported', 'success');
  } catch (error) {
    console.error('[Settings] Export failed:', error);
    showSettingsBanner(`Failed to export settings: ${error}`, 'error');
    showStatus('Export failed', 'error');
  }
}

async function handleImportFileSelected(event) {
  const file = event.target.files && event.target.files[0];
  if (!file) {
    return;
  }

  try {
    showSettingsBanner('Reading backup file...', 'info');
    const text = await file.text();

    const imported = await invoke('import_settings', { jsonContent: text });
    console.log('[Settings] Imported settings:', imported);

    // Apply the imported settings across the system, hardware, and active views
    await applyAllSettings(imported, true);

    // Refresh UI display
    updateSettingsDisplay(imported);

    showSettingsBanner('✓ Settings successfully imported and applied to hardware!', 'success');
    showStatus('✓ Settings imported & applied', 'success');
  } catch (error) {
    console.error('[Settings] Import failed:', error);
    showSettingsBanner(`Import failed: ${error}`, 'error');
    showStatus('Import failed', 'error');
  } finally {
    // Reset file input so re-selecting the same file fires change event
    event.target.value = '';
  }
}

async function handleResetSettings() {
  const confirmed = confirm(
    'Are you sure you want to reset all settings to default values?\n\nThis will restore factory defaults for fan control, battery thresholds, and performance profiles.'
  );

  if (!confirmed) {
    return;
  }

  try {
    showSettingsBanner('Resetting settings to defaults...', 'info');
    const defaults = await invoke('reset_settings');
    await applyAllSettings(defaults, true);
    updateSettingsDisplay(defaults);

    showSettingsBanner('✓ Settings have been reset to factory defaults', 'success');
    showStatus('✓ Reset to defaults', 'success');
  } catch (error) {
    console.error('[Settings] Reset failed:', error);
    showSettingsBanner(`Failed to reset settings: ${error}`, 'error');
    showStatus('Reset failed', 'error');
  }
}

function updateSettingsDisplay(settings) {
  if (!settings) {
    return;
  }

  setText('cfg-fan-mode', settings.fan_mode);
  setText('cfg-fan-level', settings.fan_level);
  setText(
    'cfg-fan-curve',
    settings.fan_curve_enabled
      ? `Enabled (${settings.fan_curve_points ? settings.fan_curve_points.length : 0} pts)`
      : 'Disabled'
  );

  setText('cfg-battery-start', `${settings.battery_start_threshold}%`);
  setText('cfg-battery-stop', `${settings.battery_stop_threshold}%`);

  setText('cfg-cpu-governor', settings.cpu_governor || 'default');
  setText('cfg-turbo-boost', settings.turbo_boost_enabled ? 'Enabled' : 'Disabled');
  setText('cfg-power-profile', settings.power_profile || 'default');

  setText('cfg-theme', settings.theme || 'system');
  setText('cfg-autostart', settings.auto_start ? 'Enabled' : 'Disabled');
  setText('cfg-minimize-tray', settings.minimize_to_tray ? 'Enabled' : 'Disabled');
}

function setText(id, value) {
  const el = document.getElementById(id);
  if (el) {
    el.textContent = value !== undefined && value !== null ? String(value) : '--';
  }
}

function showSettingsBanner(message, type = 'info') {
  const banner = document.getElementById('settings-status-message');
  if (!banner) {
    return;
  }

  if (statusTimeout) {
    clearTimeout(statusTimeout);
  }

  banner.textContent = message;
  banner.className = `settings-status-banner ${type}`;
  banner.style.display = 'block';

  // Auto-hide success or info banners after 6 seconds
  if (type === 'success' || type === 'info') {
    statusTimeout = setTimeout(() => {
      banner.style.display = 'none';
    }, 6000);
  }
}
