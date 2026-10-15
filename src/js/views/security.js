import { LogPanel } from '../logPanel.js';
import { escapeHtml } from '../utils.js';

// Security View - Biometrics (Windows Hello) and Antivirus
const { invoke } = window.__TAURI__.core;

const scanLogs = new LogPanel('scan-logs', 'btn-toggle-scan-logs', 30);
const installLogs = new LogPanel('install-logs', 'btn-toggle-install-logs', 50);

const showScanLogs = (scanType) => scanLogs.start(scanType);
const updateScanLogs = (logs) => scanLogs.update(logs);
const completeScanLogs = (ok) => scanLogs.complete(ok);

const showInstallLogs = () => installLogs.start('Installing ClamAV');
const updateInstallLogs = (logs) => installLogs.update(logs);
const completeInstallLogs = (ok) => installLogs.complete(ok);

let scanInProgress = false;
let currentBiometricData = null;

export async function loadSecurityStatus() {
  console.log('[Security] Loading biometrics and security status...');

  await Promise.allSettled([loadBiometricsStatus(), loadClamavStatus()]);
}

async function loadClamavStatus() {
  try {
    const response = await invoke('get_security_status');

    if (response.success && response.data) {
      updateSecurityUI(response.data);
    } else {
      console.error('[Security] Failed to load ClamAV status:', response.error);
    }
  } catch (error) {
    console.error('[Security] Error loading ClamAV status:', error);
  }
}

export async function loadBiometricsStatus() {
  try {
    const overview = await invoke('get_biometric_overview');
    currentBiometricData = overview;
    updateBiometricsUI(overview);
  } catch (error) {
    console.error('[Security] Error loading biometric status:', error);
  }
}

function updateBiometricsUI(data) {
  if (!data) {
    return;
  }

  const { fingerprint, face_recognition: face } = data;

  // --- 1. Status Chips ---
  const chipFpDesc = document.getElementById('chip-fingerprint-desc');
  const chipFpBadge = document.getElementById('chip-fingerprint-badge');
  if (chipFpDesc && chipFpBadge) {
    if (fingerprint.device && fingerprint.device.found) {
      chipFpDesc.textContent = fingerprint.device.name;
      chipFpBadge.className = 'chip-badge badge-success';
      chipFpBadge.textContent = 'Detected';
    } else {
      chipFpDesc.textContent = 'No sensor found';
      chipFpBadge.className = 'chip-badge badge-neutral';
      chipFpBadge.textContent = 'Not Found';
    }
  }

  const chipCamDesc = document.getElementById('chip-camera-desc');
  const chipCamBadge = document.getElementById('chip-camera-badge');
  if (chipCamDesc && chipCamBadge) {
    if (face.ir_camera_found) {
      chipCamDesc.textContent = `IR Camera (${face.recommended_camera || '/dev/video2'})`;
      chipCamBadge.className = 'chip-badge badge-success';
      chipCamBadge.textContent = 'IR Ready';
    } else {
      chipCamDesc.textContent = face.cameras.length > 0 ? 'Standard RGB Only' : 'No camera found';
      chipCamBadge.className = 'chip-badge badge-neutral';
      chipCamBadge.textContent = 'No IR';
    }
  }

  const chipPamDesc = document.getElementById('chip-pam-desc');
  const chipPamBadge = document.getElementById('chip-pam-badge');
  if (chipPamDesc && chipPamBadge) {
    if (fingerprint.pam_auth_enabled || face.pam_auth_enabled) {
      chipPamDesc.textContent = 'Active for Sudo & Login';
      chipPamBadge.className = 'chip-badge badge-success';
      chipPamBadge.textContent = 'Active';
    } else {
      chipPamDesc.textContent = 'Standard Password Auth';
      chipPamBadge.className = 'chip-badge badge-neutral';
      chipPamBadge.textContent = 'Standard';
    }
  }

  // --- 2. Fingerprint Section ---
  const fpServiceBadge = document.getElementById('fp-service-badge');
  const fpDevDesc = document.getElementById('fp-device-description');
  const btnInstallFprintd = document.getElementById('btn-install-fprintd');
  const fpEnrolledList = document.getElementById('fp-enrolled-list');
  const fpEnrolledCount = document.getElementById('fp-enrolled-count');

  if (fpServiceBadge) {
    fpServiceBadge.className = 'extension-badge';
    if (!fingerprint.device.found) {
      fpServiceBadge.classList.add('badge-neutral');
      fpServiceBadge.textContent = 'No Sensor';
    } else if (!fingerprint.fprintd_installed) {
      fpServiceBadge.classList.add('badge-warning');
      fpServiceBadge.textContent = 'Driver Missing';
    } else if (fingerprint.enrolled_fingers.length > 0) {
      fpServiceBadge.classList.add('badge-success');
      fpServiceBadge.textContent = 'Enrolled & Ready';
    } else {
      fpServiceBadge.classList.add('badge-neutral');
      fpServiceBadge.textContent = 'Sensor Ready';
    }
  }

  if (fpDevDesc && fingerprint.device.found) {
    fpDevDesc.textContent = `Hardware: ${fingerprint.device.name}. Scan your finger to authenticate passwordlessly.`;
  }

  if (btnInstallFprintd) {
    btnInstallFprintd.style.display = fingerprint.fprintd_installed ? 'none' : 'inline-flex';
  }

  if (fpEnrolledCount) {
    const count = fingerprint.enrolled_fingers.length;
    fpEnrolledCount.textContent = `${count} ${count === 1 ? 'finger' : 'fingers'}`;
  }

  if (fpEnrolledList) {
    if (fingerprint.enrolled_fingers.length === 0) {
      fpEnrolledList.innerHTML =
        '<div class="bio-empty-placeholder">No fingerprints registered yet. Select a finger below to enroll.</div>';
    } else {
      fpEnrolledList.innerHTML = fingerprint.enrolled_fingers
        .map(
          (f) => `
            <div class="bio-finger-item" data-id="${escapeHtml(f.id)}">
              <span>👆 ${escapeHtml(f.label)}</span>
              <button class="btn-del-finger" data-id="${escapeHtml(f.id)}" title="Delete fingerprint">&times;</button>
            </div>
          `
        )
        .join('');

      fpEnrolledList.querySelectorAll('.btn-del-finger').forEach((btn) => {
        btn.addEventListener('click', () => handleDeleteFinger(btn.dataset.id));
      });
    }
  }

  // --- 3. Facial Recognition Section ---
  const faceEngineBadge = document.getElementById('face-engine-badge');
  const btnInstallHowdy = document.getElementById('btn-install-howdy');
  const selectCamera = document.getElementById('select-ir-camera');
  const faceModelsList = document.getElementById('face-models-list');
  const faceModelsCount = document.getElementById('face-models-count');

  if (faceEngineBadge) {
    faceEngineBadge.className = 'extension-badge';
    if (!face.howdy_installed) {
      faceEngineBadge.classList.add('badge-warning');
      faceEngineBadge.textContent = 'Engine Missing';
    } else if (face.models.length > 0) {
      faceEngineBadge.classList.add('badge-success');
      faceEngineBadge.textContent = 'Face Models Ready';
    } else {
      faceEngineBadge.classList.add('badge-neutral');
      faceEngineBadge.textContent = 'Ready to Enroll';
    }
  }

  if (btnInstallHowdy) {
    btnInstallHowdy.style.display = face.howdy_installed ? 'none' : 'inline-flex';
  }

  if (selectCamera && face.cameras.length > 0) {
    selectCamera.innerHTML = face.cameras
      .map((cam) => {
        const isSelected =
          (face.configured_device && face.configured_device === cam.path) ||
          (!face.configured_device && cam.is_infrared);
        const tag = cam.is_infrared ? ' [Infrared - Recommended]' : ' [Color RGB]';
        return `<option value="${escapeHtml(cam.path)}"${isSelected ? ' selected' : ''}>${escapeHtml(cam.path)} — ${escapeHtml(cam.name)}${tag}</option>`;
      })
      .join('');
  }

  if (faceModelsCount) {
    const count = face.models.length;
    faceModelsCount.textContent = `${count} ${count === 1 ? 'profile' : 'profiles'}`;
  }

  if (faceModelsList) {
    if (face.models.length === 0) {
      faceModelsList.innerHTML =
        '<div class="bio-empty-placeholder">No face models registered yet. Add a profile below.</div>';
    } else {
      faceModelsList.innerHTML = face.models
        .map(
          (m) => `
            <div class="bio-face-item" data-id="${escapeHtml(m.id)}">
              <span>👤 ${escapeHtml(m.label)} (${escapeHtml(m.date)})</span>
              <button class="btn-del-face" data-id="${escapeHtml(m.id)}" title="Delete face model">&times;</button>
            </div>
          `
        )
        .join('');

      faceModelsList.querySelectorAll('.btn-del-face').forEach((btn) => {
        btn.addEventListener('click', () => handleDeleteFaceModel(btn.dataset.id));
      });
    }
  }
}

async function handleEnrollFinger() {
  const select = document.getElementById('select-enroll-finger');
  const finger = select ? select.value : 'right-index-finger';
  const banner = document.getElementById('fp-action-banner');

  if (banner) {
    banner.style.display = 'block';
    banner.className = 'settings-status-banner info';
    banner.textContent = `Place and swipe your finger (${finger}) on the Goodix sensor repeatedly when prompted...`;
  }

  showNotification('Touch the fingerprint sensor to begin enrollment', 'info');

  try {
    const btn = document.getElementById('btn-enroll-finger');
    if (btn) {
      btn.disabled = true;
      btn.textContent = 'Enrolling...';
    }

    // Launch enrollment in terminal or fprintd
    const response = await invoke('install_fingerprint_packages');
    if (banner) {
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message || 'Fingerprint enrollment initialized.';
    }

    await loadBiometricsStatus();
  } catch (err) {
    console.error('[Biometrics] Enroll error:', err);
    if (banner) {
      banner.className = 'settings-status-banner error';
      banner.textContent = `Enrollment error: ${err}`;
    }
  } finally {
    const btn = document.getElementById('btn-enroll-finger');
    if (btn) {
      btn.disabled = false;
      btn.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg> Enroll Finger';
    }
  }
}

async function handleDeleteFinger(fingerId) {
  const banner = document.getElementById('fp-action-banner');
  try {
    const response = await invoke('delete_fingerprint', { finger: fingerId });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification(response.message, response.success ? 'success' : 'error');
    await loadBiometricsStatus();
  } catch (err) {
    showNotification(`Delete failed: ${err}`, 'error');
  }
}

async function handleClearAllFingers() {
  const confirmed = confirm('Are you sure you want to remove all enrolled fingerprints?');
  if (!confirmed) {
    return;
  }

  const banner = document.getElementById('fp-action-banner');
  try {
    const response = await invoke('delete_fingerprint', { finger: null });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification('All fingerprints deleted', 'success');
    await loadBiometricsStatus();
  } catch (err) {
    showNotification(`Clear failed: ${err}`, 'error');
  }
}

async function handleVerifyFinger() {
  const banner = document.getElementById('fp-action-banner');
  if (banner) {
    banner.style.display = 'block';
    banner.className = 'settings-status-banner info';
    banner.textContent = 'Verification active: touch the fingerprint sensor now...';
  }
  showNotification('Touch the fingerprint sensor to verify...', 'info');
}

async function handleInstallFprintd() {
  const banner = document.getElementById('fp-action-banner');
  const btn = document.getElementById('btn-install-fprintd');
  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Installing...';
  }

  if (banner) {
    banner.style.display = 'block';
    banner.className = 'settings-status-banner info';
    banner.textContent = 'Installing fprintd & libpam-fprintd packages...';
  }

  try {
    const response = await invoke('install_fingerprint_packages');
    if (banner) {
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification(response.message, response.success ? 'success' : 'error');
    await loadBiometricsStatus();
  } catch (err) {
    if (banner) {
      banner.className = 'settings-status-banner error';
      banner.textContent = `Installation failed: ${err}`;
    }
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.textContent = 'Install fprintd';
    }
  }
}

async function handleSetCamera() {
  const select = document.getElementById('select-ir-camera');
  const banner = document.getElementById('face-action-banner');
  const devicePath = select ? select.value : '';

  if (!devicePath) {
    showNotification('Please select a camera device', 'error');
    return;
  }

  try {
    const response = await invoke('configure_howdy_camera', { devicePath });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification(response.message, response.success ? 'success' : 'error');
    await loadBiometricsStatus();
  } catch (err) {
    showNotification(`Failed to configure camera: ${err}`, 'error');
  }
}

async function handleAddFaceModel() {
  const input = document.getElementById('input-face-label');
  const banner = document.getElementById('face-action-banner');
  const label = input && input.value.trim() ? input.value.trim() : 'ThinkPad User';

  if (banner) {
    banner.style.display = 'block';
    banner.className = 'settings-status-banner info';
    banner.textContent = `Looking at the camera... Capturing face profile "${label}"...`;
  }
  showNotification('Look directly into the IR camera...', 'info');

  try {
    const response = await invoke('configure_howdy_camera', {
      devicePath: currentBiometricData?.face_recognition?.recommended_camera || '/dev/video2'
    });
    if (banner) {
      banner.className = 'settings-status-banner success';
      banner.textContent = response.message;
    }
    if (input) {
      input.value = '';
    }
    await loadBiometricsStatus();
  } catch (err) {
    if (banner) {
      banner.className = 'settings-status-banner error';
      banner.textContent = `Face capture error: ${err}`;
    }
  }
}

async function handleDeleteFaceModel(id) {
  const banner = document.getElementById('face-action-banner');
  try {
    const response = await invoke('remove_face_model', { id });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification(response.message, response.success ? 'success' : 'error');
    await loadBiometricsStatus();
  } catch (err) {
    showNotification(`Failed to remove face model: ${err}`, 'error');
  }
}

async function handleClearFaceModels() {
  const confirmed = confirm('Are you sure you want to remove all face recognition profiles?');
  if (!confirmed) {
    return;
  }

  const banner = document.getElementById('face-action-banner');
  try {
    const response = await invoke('clear_face_models');
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification('All face models cleared', 'success');
    await loadBiometricsStatus();
  } catch (err) {
    showNotification(`Failed to clear face models: ${err}`, 'error');
  }
}

function updateSecurityUI(status) {
  // Update ClamAV status
  const statusBadge = document.getElementById('clamav-status-badge');
  const statusText = document.getElementById('clamav-status-text');
  const installSection = document.getElementById('clamav-install-section');
  const controlsSection = document.getElementById('security-controls-section');

  if (status.clamav_installed) {
    if (statusBadge) {
      statusBadge.className = 'badge badge-success';
      statusBadge.textContent = 'Installed';
    }
    if (statusText) {
      statusText.textContent = status.clamav_running ? 'Running' : 'Stopped';
    }
    if (installSection) {
      installSection.style.display = 'none';
    }
    if (controlsSection) {
      controlsSection.style.display = 'block';
    }

    // Update database info
    updateElement('db-version', status.database_version);
    updateElement('db-last-update', status.last_update);
    updateElement('db-definitions', status.definitions_count);
  } else {
    if (statusBadge) {
      statusBadge.className = 'badge badge-warning';
      statusBadge.textContent = 'Not Installed';
    }
    if (statusText) {
      statusText.textContent = 'Not Available';
    }
    if (installSection) {
      installSection.style.display = 'flex';
    }
    if (controlsSection) {
      controlsSection.style.display = 'none';
    }
  }
}

function updateElement(id, value) {
  const element = document.getElementById(id);
  if (element) {
    element.textContent = value || 'N/A';
  }
}

export function setupSecurityHandlers() {
  // Sub-navigation tabs
  const tabBiometrics = document.getElementById('tab-btn-biometrics');
  const tabAntivirus = document.getElementById('tab-btn-antivirus');
  const panelBiometrics = document.getElementById('security-panel-biometrics');
  const panelAntivirus = document.getElementById('security-panel-antivirus');

  if (tabBiometrics && tabAntivirus && panelBiometrics && panelAntivirus) {
    tabBiometrics.addEventListener('click', () => {
      tabBiometrics.classList.add('active');
      tabAntivirus.classList.remove('active');
      panelBiometrics.style.display = 'flex';
      panelAntivirus.style.display = 'none';
    });

    tabAntivirus.addEventListener('click', () => {
      tabAntivirus.classList.add('active');
      tabBiometrics.classList.remove('active');
      panelAntivirus.style.display = 'flex';
      panelBiometrics.style.display = 'none';
    });
  }

  // Biometrics buttons
  const btnRefreshBio = document.getElementById('btn-refresh-biometrics');
  if (btnRefreshBio) {
    btnRefreshBio.addEventListener('click', async () => {
      await loadBiometricsStatus();
      showNotification('Biometric status refreshed', 'success');
    });
  }

  const btnEnrollFp = document.getElementById('btn-enroll-finger');
  if (btnEnrollFp) {
    btnEnrollFp.addEventListener('click', handleEnrollFinger);
  }

  const btnVerifyFp = document.getElementById('btn-verify-finger');
  if (btnVerifyFp) {
    btnVerifyFp.addEventListener('click', handleVerifyFinger);
  }

  const btnClearFp = document.getElementById('btn-clear-all-fingers');
  if (btnClearFp) {
    btnClearFp.addEventListener('click', handleClearAllFingers);
  }

  const btnInstallFp = document.getElementById('btn-install-fprintd');
  if (btnInstallFp) {
    btnInstallFp.addEventListener('click', handleInstallFprintd);
  }

  const btnSetCam = document.getElementById('btn-set-camera');
  if (btnSetCam) {
    btnSetCam.addEventListener('click', handleSetCamera);
  }

  const btnAddFace = document.getElementById('btn-add-face-model');
  if (btnAddFace) {
    btnAddFace.addEventListener('click', handleAddFaceModel);
  }

  const btnClearFace = document.getElementById('btn-clear-face-models');
  if (btnClearFace) {
    btnClearFace.addEventListener('click', handleClearFaceModels);
  }

  // Refresh status button for ClamAV
  const refreshBtn = document.getElementById('btn-refresh-status');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', async () => {
      refreshBtn.disabled = true;
      const originalText = refreshBtn.innerHTML;
      refreshBtn.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="animation: spin 1s linear infinite;"><path d="M21.5 2v6h-6M2.5 22v-6h6M2 11.5a10 10 0 0 1 18.8-4.3M22 12.5a10 10 0 0 1-18.8 4.2" /></svg> Refreshing...';

      try {
        await loadSecurityStatus();
        showNotification('Status refreshed', 'success');
      } catch (error) {
        showNotification('Failed to refresh status', 'error');
      } finally {
        refreshBtn.disabled = false;
        refreshBtn.innerHTML = originalText;
      }
    });
  }

  // Install ClamAV
  const installBtn = document.getElementById('btn-install-clamav');
  if (installBtn) {
    installBtn.addEventListener('click', async () => {
      installBtn.disabled = true;
      installBtn.textContent = 'Installing...';

      // Show inline installation logs
      showInstallLogs();

      try {
        const response = await invoke('install_clamav');

        // Update logs inline
        if (response.logs && response.logs.length > 0) {
          updateInstallLogs(response.logs);
        }

        if (response.success) {
          showNotification('ClamAV installed successfully', 'success');
          completeInstallLogs(true);
          setTimeout(async () => {
            await loadSecurityStatus();
          }, 2000);
        } else {
          // Check if it's a manual install error
          if (response.error && response.error.startsWith('MANUAL_INSTALL:')) {
            hideInstallLogs();
            const instructions = response.error.replace('MANUAL_INSTALL:', '');
            showManualInstallDialog(instructions);
          } else {
            completeInstallLogs(false);
            showNotification('Installation failed: ' + response.error, 'error');
          }
        }
      } catch (error) {
        completeInstallLogs(false);
        showNotification('Error: ' + error, 'error');
      } finally {
        installBtn.disabled = false;
        installBtn.textContent = 'Install ClamAV';
      }
    });
  }

  // Update definitions
  const updateBtn = document.getElementById('btn-update-definitions');
  if (updateBtn) {
    updateBtn.addEventListener('click', async () => {
      updateBtn.disabled = true;
      const originalText = updateBtn.textContent;
      updateBtn.textContent = 'Updating...';

      try {
        const response = await invoke('update_virus_definitions');

        if (response.success) {
          showNotification('Virus definitions updated', 'success');
          await loadSecurityStatus();
        } else {
          showNotification('Update failed: ' + response.error, 'error');
        }
      } catch (error) {
        showNotification('Error: ' + error, 'error');
      } finally {
        updateBtn.disabled = false;
        updateBtn.textContent = originalText;
      }
    });
  }

  // Quick scan
  const quickScanBtn = document.getElementById('btn-quick-scan');
  if (quickScanBtn) {
    quickScanBtn.addEventListener('click', async () => {
      if (scanInProgress) {
        return;
      }

      scanInProgress = true;
      quickScanBtn.disabled = true;
      quickScanBtn.textContent = 'Scanning...';

      // Show inline scan logs
      showScanLogs('Quick Scan');

      try {
        const response = await invoke('quick_scan');

        if (response.success && response.data) {
          // Update logs inline
          if (response.data.logs && response.data.logs.length > 0) {
            updateScanLogs(response.data.logs);
          }

          displayScanResults(response.data);
          completeScanLogs(true);
          showNotification('Quick scan completed', 'success');
        } else {
          completeScanLogs(false);
          showNotification('Scan failed: ' + response.error, 'error');
        }
      } catch (error) {
        completeScanLogs(false);
        showNotification('Error: ' + error, 'error');
      } finally {
        scanInProgress = false;
        quickScanBtn.disabled = false;
        quickScanBtn.textContent = 'Quick Scan';
      }
    });
  }

  // Custom scan
  const customScanBtn = document.getElementById('btn-custom-scan');
  if (customScanBtn) {
    customScanBtn.addEventListener('click', async () => {
      const pathInput = document.getElementById('scan-path-input');
      const path = pathInput?.value.trim();

      if (!path) {
        showNotification('Please enter a path to scan', 'error');
        return;
      }

      if (scanInProgress) {
        return;
      }

      scanInProgress = true;
      customScanBtn.disabled = true;
      customScanBtn.textContent = 'Scanning...';

      // Show inline scan logs
      showScanLogs('Custom Scan');

      try {
        const response = await invoke('scan_path', { path });

        if (response.success && response.data) {
          // Update logs inline
          if (response.data.logs && response.data.logs.length > 0) {
            updateScanLogs(response.data.logs);
          }

          displayScanResults(response.data);
          completeScanLogs(true);
          showNotification('Scan completed', 'success');
        } else {
          completeScanLogs(false);
          showNotification('Scan failed: ' + response.error, 'error');
        }
      } catch (error) {
        completeScanLogs(false);
        showNotification('Error: ' + error, 'error');
      } finally {
        scanInProgress = false;
        customScanBtn.disabled = false;
        customScanBtn.textContent = 'Scan Path';
      }
    });
  }

  // Browse path button
  const browseBtn = document.getElementById('btn-browse-path');
  if (browseBtn) {
    browseBtn.addEventListener('click', async () => {
      // In a real implementation with tauri-plugin-dialog:
      // const selected = await open({ directory: true, multiple: false });
      // For now, prompt the user or set default
      const pathInput = document.getElementById('scan-path-input');
      const current = pathInput?.value || '/home';
      const path = prompt('Enter path to scan:', current);
      if (path && pathInput) {
        pathInput.value = path;
      }
    });
  }
}

function displayScanResults(result) {
  const resultsCard = document.getElementById('scan-results');
  const threatStatus = document.getElementById('scan-threat-status');
  const filesCount = document.getElementById('scan-files-count');
  const threatsCount = document.getElementById('scan-threats-count');
  const scanTime = document.getElementById('scan-time');
  const threatsList = document.getElementById('threats-list');

  if (resultsCard) {
    resultsCard.style.display = 'block';
  }

  if (threatStatus) {
    if (result.infected_files > 0) {
      threatStatus.className = 'badge badge-danger';
      threatStatus.textContent = `${result.infected_files} Threats Detected`;
    } else {
      threatStatus.className = 'badge badge-success';
      threatStatus.textContent = 'Clean';
    }
  }

  if (filesCount) {
    filesCount.textContent = result.scanned_files;
  }
  if (threatsCount) {
    threatsCount.textContent = result.infected_files;
  }
  if (scanTime) {
    scanTime.textContent = result.scan_time;
  }

  if (threatsList) {
    if (result.threats && result.threats.length > 0) {
      threatsList.innerHTML = result.threats
        .map(
          (threat) => `
        <div class="threat-item">
          <div class="threat-icon">⚠️</div>
          <div class="threat-info">
            <div class="threat-name">${escapeHtml(threat.threat_name)}</div>
            <div class="threat-path">${escapeHtml(threat.file_path)}</div>
          </div>
        </div>
      `
        )
        .join('');
    } else {
      threatsList.innerHTML = '<div class="no-threats">No threats detected</div>';
    }
  }
}

function showNotification(message, type = 'info') {
  // Use existing notification system or create banner
  console.log(`[Security Notification] [${type}]: ${message}`);

  const notification = document.createElement('div');
  notification.className = `security-notification ${type}`;
  notification.textContent = message;

  document.body.appendChild(notification);

  setTimeout(() => {
    notification.classList.add('show');
  }, 10);

  setTimeout(() => {
    notification.classList.remove('show');
    setTimeout(() => {
      notification.remove();
    }, 300);
  }, 3000);
}

function hideInstallLogs() {
  const logsSection = document.getElementById('install-logs-section');
  if (logsSection) {
    logsSection.style.display = 'none';
  }
}

function showManualInstallDialog(instructions) {
  const existingDialog = document.querySelector('.manual-install-dialog');
  if (existingDialog) {
    existingDialog.remove();
  }

  const dialog = document.createElement('div');
  dialog.className = 'manual-install-dialog';
  dialog.innerHTML = `
    <div class="manual-install-overlay"></div>
    <div class="manual-install-content">
      <div class="manual-install-header">
        <h3>Manual Installation Required</h3>
        <button class="manual-install-close">&times;</button>
      </div>
      <div class="manual-install-body">
        <p>Automatic installation is not supported for your Linux distribution. Please install ClamAV manually using the following command:</p>
        <div class="manual-install-command">
          <code>${escapeHtml(instructions)}</code>
          <button class="manual-install-copy" title="Copy to clipboard">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
          </button>
        </div>
        <p class="manual-install-note">After installation, click the refresh button below to update the status.</p>
      </div>
      <div class="manual-install-footer">
        <button class="security-button secondary manual-install-cancel">Close</button>
        <button class="security-button primary manual-install-refresh">Refresh Status</button>
      </div>
    </div>
  `;

  document.body.appendChild(dialog);

  const closeBtn = dialog.querySelector('.manual-install-close');
  const cancelBtn = dialog.querySelector('.manual-install-cancel');
  const overlay = dialog.querySelector('.manual-install-overlay');

  const closeDialog = () => dialog.remove();

  closeBtn.addEventListener('click', closeDialog);
  cancelBtn.addEventListener('click', closeDialog);
  overlay.addEventListener('click', closeDialog);

  const copyBtn = dialog.querySelector('.manual-install-copy');
  copyBtn.addEventListener('click', () => {
    navigator.clipboard
      .writeText(instructions)
      .then(() => {
        showNotification('Command copied to clipboard', 'success');
      })
      .catch(() => {
        showNotification('Failed to copy command', 'error');
      });
  });

  const refreshBtn = dialog.querySelector('.manual-install-refresh');
  refreshBtn.addEventListener('click', async () => {
    closeDialog();
    await loadSecurityStatus();
  });
}
