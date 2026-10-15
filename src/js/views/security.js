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

let biometricsEventsAttached = false;
async function setupBiometricsEvents() {
  if (biometricsEventsAttached || !window.__TAURI__?.event?.listen) {
    return;
  }
  biometricsEventsAttached = true;
  const { listen } = window.__TAURI__.event;

  await listen('fingerprint-enroll-progress', (event) => {
    handleFingerprintEnrollProgress(event.payload);
  });

  await listen('fingerprint-verify-progress', (event) => {
    handleFingerprintVerifyProgress(event.payload);
  });

  await listen('biometric-install-progress', (event) => {
    handleBiometricInstallProgress(event.payload);
  });
}

function resetTouchIdVisuals() {
  const stage = document.getElementById('touch-id-stage');
  if (stage) {
    stage.className = 'touch-id-stage';
  }
  const meterRing = document.getElementById('touch-id-meter-ring');
  if (meterRing) {
    meterRing.style.strokeDashoffset = '295.3';
    meterRing.style.stroke = '';
  }
  for (let i = 1; i <= 7; i++) {
    const ridge = document.getElementById(`fp-ridge-${i}`);
    if (ridge) {
      ridge.setAttribute('class', 'fp-ridge');
    }
  }
}

function updateTouchIdEnrollProgress(percent, isDone, isSuccess) {
  const clampedPercent = Math.max(0, Math.min(100, Number(percent) || 0));
  const activeRidges = Math.min(7, Math.ceil((clampedPercent / 100) * 7));

  for (let i = 1; i <= 7; i++) {
    const ridge = document.getElementById(`fp-ridge-${i}`);
    if (ridge) {
      ridge.setAttribute('class', i <= activeRidges ? 'fp-ridge active' : 'fp-ridge');
    }
  }

  const meterRing = document.getElementById('touch-id-meter-ring');
  if (meterRing) {
    const circumference = 295.3;
    meterRing.style.strokeDashoffset = String(circumference * (1 - clampedPercent / 100));
  }

  const ripple = document.getElementById('touch-id-ripple');
  if (ripple) {
    ripple.classList.remove('pulse');
    void ripple.offsetWidth;
    ripple.classList.add('pulse');
  }

  const stage = document.getElementById('touch-id-stage');
  if (stage && isDone) {
    if (isSuccess) {
      stage.className = 'touch-id-stage completed';
    } else {
      stage.className = 'touch-id-stage failed';
    }
  }
}

function updateTouchIdVerifyState(status, isDone, isSuccess) {
  const stage = document.getElementById('touch-id-stage');
  const meterRing = document.getElementById('touch-id-meter-ring');

  if (status === 'match' || (isDone && isSuccess)) {
    if (stage) {
      stage.className = 'touch-id-stage completed';
    }
    if (meterRing) {
      meterRing.style.strokeDashoffset = '0';
      meterRing.style.stroke = '#10b981';
    }
    for (let i = 1; i <= 7; i++) {
      const ridge = document.getElementById(`fp-ridge-${i}`);
      if (ridge) {
        ridge.setAttribute('class', 'fp-ridge verified');
      }
    }
  } else if (status === 'no-match' || (isDone && !isSuccess)) {
    if (stage) {
      stage.className = 'touch-id-stage failed';
    }
    if (meterRing) {
      meterRing.style.stroke = '#ef4444';
    }
  } else {
    if (stage) {
      stage.className = 'touch-id-stage';
    }
    if (meterRing) {
      meterRing.style.strokeDashoffset = '147';
      meterRing.style.stroke = '#38bdf8';
    }
    for (let i = 1; i <= 7; i++) {
      const ridge = document.getElementById(`fp-ridge-${i}`);
      if (ridge) {
        ridge.setAttribute('class', 'fp-ridge scanning');
      }
    }
  }
}

function setFaceIdState(state) {
  const stage = document.getElementById('face-id-stage');
  if (!stage) {
    return;
  }
  if (state === 'idle') {
    stage.className = 'face-id-stage';
  } else {
    stage.className = `face-id-stage ${state}`;
  }
}

function handleFingerprintEnrollProgress(payload) {
  if (!payload) {
    return;
  }
  const progressBox = document.getElementById('fp-progress-box');
  const percentEl = document.getElementById('fp-progress-percent');
  const fillEl = document.getElementById('fp-progress-bar-fill');
  const stepEl = document.getElementById('fp-progress-step');
  const banner = document.getElementById('fp-action-banner');

  if (progressBox) {
    progressBox.style.display = 'flex';
  }
  if (percentEl) {
    percentEl.textContent = `${payload.percent}%`;
  }
  if (fillEl) {
    fillEl.style.width = `${payload.percent}%`;
  }
  if (stepEl && payload.message) {
    stepEl.textContent = payload.message;
  }

  updateTouchIdEnrollProgress(payload.percent, payload.done, payload.success);

  if (payload.done) {
    if (banner) {
      banner.style.display = 'block';
      banner.className = payload.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = payload.message;
    }
  }
}

function handleFingerprintVerifyProgress(payload) {
  if (!payload) {
    return;
  }
  const progressBox = document.getElementById('fp-progress-box');
  const titleEl = document.getElementById('fp-progress-title');
  const stepEl = document.getElementById('fp-progress-step');
  const fillEl = document.getElementById('fp-progress-bar-fill');
  const percentEl = document.getElementById('fp-progress-percent');
  const banner = document.getElementById('fp-action-banner');

  if (progressBox) {
    progressBox.style.display = 'flex';
  }
  if (titleEl) {
    titleEl.textContent = 'Verify Fingerprint';
  }
  if (percentEl) {
    percentEl.textContent = payload.status === 'match' ? '100%' : '';
  }
  if (fillEl) {
    fillEl.style.width = payload.status === 'match' ? '100%' : '60%';
  }
  if (stepEl && payload.message) {
    stepEl.textContent = payload.message;
  }

  updateTouchIdVerifyState(payload.status, payload.done, payload.success);

  if (payload.done) {
    if (banner) {
      banner.style.display = 'block';
      banner.className = payload.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = payload.message;
    }
  }
}

function handleBiometricInstallProgress(payload) {
  if (!payload) {
    return;
  }
  const isFace = payload.target === 'face';
  const prefix = isFace ? 'face' : 'fp';

  const installBox = document.getElementById(`${prefix}-install-box`);
  const percentEl = document.getElementById(`${prefix}-install-percent`);
  const fillEl = document.getElementById(`${prefix}-install-bar-fill`);
  const stepEl = document.getElementById(`${prefix}-install-step`);
  const logsEl = document.getElementById(`${prefix}-install-logs-output`);
  const spinner = document.getElementById(`${prefix}-install-spinner`);

  if (installBox) {
    installBox.style.display = 'flex';
  }
  if (percentEl) {
    percentEl.textContent = `${payload.percent}%`;
  }
  if (fillEl) {
    fillEl.style.width = `${payload.percent}%`;
  }
  if (stepEl && payload.stage) {
    stepEl.textContent = payload.stage;
  }

  if (logsEl && payload.log_line) {
    const line = document.createElement('div');
    line.className = 'bio-log-line';
    if (payload.log_line.includes('✓') || payload.log_line.includes('ALL_DONE')) {
      line.style.color = '#10b981';
    } else if (payload.log_line.includes('ERR') || payload.log_line.includes('✗')) {
      line.style.color = '#ef4444';
    } else if (payload.log_line.startsWith('===')) {
      line.style.color = '#38bdf8';
      line.style.fontWeight = 'bold';
    }
    line.textContent = payload.log_line;
    logsEl.appendChild(line);
    logsEl.scrollTop = logsEl.scrollHeight;
  }

  if (payload.done) {
    if (spinner) {
      spinner.style.display = 'none';
    }
    showNotification(
      payload.stage || 'Installation finished',
      payload.success ? 'success' : 'error'
    );
    if (payload.success) {
      loadBiometricsStatus();
      setTimeout(() => {
        if (installBox) {
          installBox.style.display = 'none';
        }
      }, 4000);
    }
  }
}

async function handleEnrollFinger() {
  const select = document.getElementById('select-enroll-finger');
  const finger = select ? select.value : 'right-index-finger';
  const banner = document.getElementById('fp-action-banner');
  const progressBox = document.getElementById('fp-progress-box');
  const titleEl = document.getElementById('fp-progress-title');
  const percentEl = document.getElementById('fp-progress-percent');
  const fillEl = document.getElementById('fp-progress-bar-fill');
  const stepEl = document.getElementById('fp-progress-step');
  const btn = document.getElementById('btn-enroll-finger');

  resetTouchIdVisuals();

  if (progressBox) {
    progressBox.style.display = 'flex';
  }
  if (titleEl) {
    titleEl.textContent = `Enrolling ${finger.replace(/-/g, ' ')}`;
  }
  if (percentEl) {
    percentEl.textContent = '0%';
  }
  if (fillEl) {
    fillEl.style.width = '0%';
  }
  if (stepEl) {
    stepEl.textContent =
      'Connecting to Goodix fingerprint reader... Place and lift your finger when prompted.';
  }

  if (banner) {
    banner.style.display = 'none';
  }

  showNotification('Place and lift your finger on the sensor repeatedly', 'info');

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Enrolling...';
  }

  try {
    const response = await invoke('enroll_fingerprint', { finger });
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
    console.error('[Biometrics] Enroll error:', err);
    updateTouchIdEnrollProgress(0, true, false);
    if (banner) {
      banner.style.display = 'block';
      banner.className = 'settings-status-banner error';
      banner.textContent = `Enrollment error: ${err}`;
    }
    showNotification(`Enrollment error: ${err}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg> Enroll Finger';
    }
    setTimeout(() => {
      if (progressBox && percentEl?.textContent === '100%') {
        progressBox.style.display = 'none';
        resetTouchIdVisuals();
      }
    }, 4000);
  }
}

async function handleCancelFp() {
  try {
    await invoke('cancel_fingerprint_action');
    const progressBox = document.getElementById('fp-progress-box');
    if (progressBox) {
      progressBox.style.display = 'none';
    }
    resetTouchIdVisuals();
    showNotification('Fingerprint action cancelled', 'info');
  } catch (err) {
    console.error('[Biometrics] Cancel error:', err);
  }
}

async function handleVerifyFinger() {
  const banner = document.getElementById('fp-action-banner');
  const progressBox = document.getElementById('fp-progress-box');
  const titleEl = document.getElementById('fp-progress-title');
  const percentEl = document.getElementById('fp-progress-percent');
  const fillEl = document.getElementById('fp-progress-bar-fill');
  const stepEl = document.getElementById('fp-progress-step');
  const btn = document.getElementById('btn-verify-finger');

  resetTouchIdVisuals();
  updateTouchIdVerifyState('waiting', false, false);

  if (progressBox) {
    progressBox.style.display = 'flex';
  }
  if (titleEl) {
    titleEl.textContent = 'Verify Fingerprint';
  }
  if (percentEl) {
    percentEl.textContent = '';
  }
  if (fillEl) {
    fillEl.style.width = '50%';
  }
  if (stepEl) {
    stepEl.textContent = 'Touch the sensor now to verify...';
  }

  if (banner) {
    banner.style.display = 'none';
  }

  showNotification('Touch the fingerprint sensor to verify...', 'info');

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Waiting for touch...';
  }

  try {
    const response = await invoke('verify_fingerprint', { finger: null });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    updateTouchIdVerifyState(response.success ? 'match' : 'no-match', true, response.success);
    showNotification(response.message, response.success ? 'success' : 'error');
  } catch (err) {
    console.error('[Biometrics] Verify error:', err);
    updateTouchIdVerifyState('error', true, false);
    if (banner) {
      banner.style.display = 'block';
      banner.className = 'settings-status-banner error';
      banner.textContent = `Verification error: ${err}`;
    }
    showNotification(`Verification error: ${err}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg> Verify';
    }
    setTimeout(() => {
      if (progressBox) {
        progressBox.style.display = 'none';
        resetTouchIdVisuals();
      }
    }, 4000);
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

async function handleInstallFprintd() {
  const banner = document.getElementById('fp-action-banner');
  const btn = document.getElementById('btn-install-fprintd');
  const installBox = document.getElementById('fp-install-box');
  const fillEl = document.getElementById('fp-install-bar-fill');
  const percentEl = document.getElementById('fp-install-percent');
  const stepEl = document.getElementById('fp-install-step');
  const logsEl = document.getElementById('fp-install-logs-output');
  const spinner = document.getElementById('fp-install-spinner');

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Installing...';
  }

  if (installBox) {
    installBox.style.display = 'flex';
  }
  if (percentEl) {
    percentEl.textContent = '0%';
  }
  if (fillEl) {
    fillEl.style.width = '0%';
  }
  if (stepEl) {
    stepEl.textContent = 'Requesting authorization to install fprintd...';
  }
  if (logsEl) {
    logsEl.innerHTML = '';
  }
  if (spinner) {
    spinner.style.display = 'inline-block';
  }

  if (banner) {
    banner.style.display = 'none';
  }

  showNotification('Starting fprintd installation...', 'info');

  try {
    const response = await invoke('install_fingerprint_packages');
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
    console.error('[Biometrics] Install error:', err);
    if (banner) {
      banner.style.display = 'block';
      banner.className = 'settings-status-banner error';
      banner.textContent = `Installation failed: ${err}`;
    }
    showNotification(`Installation failed: ${err}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.textContent = 'Install fprintd';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
  }
}

async function handleInstallHowdy() {
  const select = document.getElementById('select-ir-camera');
  const irCamera = select ? select.value : null;
  const banner = document.getElementById('face-action-banner');
  const btn = document.getElementById('btn-install-howdy');
  const installBox = document.getElementById('face-install-box');
  const fillEl = document.getElementById('face-install-bar-fill');
  const percentEl = document.getElementById('face-install-percent');
  const stepEl = document.getElementById('face-install-step');
  const logsEl = document.getElementById('face-install-logs-output');
  const spinner = document.getElementById('face-install-spinner');

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Installing...';
  }

  if (installBox) {
    installBox.style.display = 'flex';
  }
  if (percentEl) {
    percentEl.textContent = '0%';
  }
  if (fillEl) {
    fillEl.style.width = '0%';
  }
  if (stepEl) {
    stepEl.textContent = 'Requesting authorization to install Howdy...';
  }
  if (logsEl) {
    logsEl.innerHTML = '';
  }
  if (spinner) {
    spinner.style.display = 'inline-block';
  }

  if (banner) {
    banner.style.display = 'none';
  }

  showNotification('Starting Howdy installation...', 'info');

  try {
    const response = await invoke('install_howdy_packages', { irCamera });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    showNotification(response.message, response.success ? 'success' : 'error');
    await loadBiometricsStatus();
    if (response.success) {
      if (btn) {
        btn.style.display = 'none';
      }
      setTimeout(() => {
        if (installBox) {
          installBox.style.display = 'none';
        }
      }, 4000);
    }
  } catch (err) {
    console.error('[Biometrics] Howdy install error:', err);
    if (banner) {
      banner.style.display = 'block';
      banner.className = 'settings-status-banner error';
      banner.textContent = `Installation failed: ${err}`;
    }
    showNotification(`Installation failed: ${err}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.textContent = 'Install Howdy';
    }
    if (spinner) {
      spinner.style.display = 'none';
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
  const select = document.getElementById('select-ir-camera');
  const banner = document.getElementById('face-action-banner');
  const progressBox = document.getElementById('face-progress-box');
  const stepEl = document.getElementById('face-progress-step');
  const btn = document.getElementById('btn-add-face-model');

  const label = input && input.value.trim() ? input.value.trim() : 'ThinkPad User';
  const cameraDevice = select ? select.value : null;

  if (progressBox) {
    progressBox.style.display = 'flex';
  }
  setFaceIdState('scanning');
  if (stepEl) {
    stepEl.textContent = `Capturing facial profile "${label}"... Look directly at the IR sensor.`;
  }
  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Scanning...';
  }

  if (banner) {
    banner.style.display = 'none';
  }

  showNotification('Look directly into the IR camera...', 'info');

  try {
    const response = await invoke('add_face_model', { label, cameraDevice });
    if (banner) {
      banner.style.display = 'block';
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    if (response.success) {
      setFaceIdState('completed');
      if (stepEl) {
        stepEl.textContent = 'Face profile recorded successfully!';
      }
    } else {
      setFaceIdState('failed');
      if (stepEl) {
        stepEl.textContent = 'Face capture was not completed.';
      }
    }
    showNotification(response.message, response.success ? 'success' : 'error');
    if (input) {
      input.value = '';
    }
    await loadBiometricsStatus();
  } catch (err) {
    console.error('[Biometrics] Add face model error:', err);
    setFaceIdState('failed');
    if (banner) {
      banner.style.display = 'block';
      banner.className = 'settings-status-banner error';
      banner.textContent = `Face capture error: ${err}`;
    }
    if (stepEl) {
      stepEl.textContent = `Face capture error: ${err}`;
    }
    showNotification(`Face capture error: ${err}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg> Add Face Profile';
    }
    setTimeout(() => {
      if (progressBox) {
        progressBox.style.display = 'none';
        setFaceIdState('idle');
      }
    }, 4000);
  }
}

async function handleTestFace() {
  const banner = document.getElementById('face-action-banner');
  const progressBox = document.getElementById('face-progress-box');
  const stepEl = document.getElementById('face-progress-step');
  const btn = document.getElementById('btn-test-face');

  if (progressBox) {
    progressBox.style.display = 'flex';
  }
  setFaceIdState('scanning');
  if (stepEl) {
    stepEl.textContent = 'Testing IR camera recognition... Look directly at the sensor.';
  }

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Testing...';
  }

  if (banner) {
    banner.style.display = 'block';
    banner.className = 'settings-status-banner info';
    banner.textContent = 'Testing IR camera recognition... Look at the screen.';
  }

  showNotification('Testing face recognition...', 'info');

  try {
    const response = await invoke('test_face_model');
    if (banner) {
      banner.className = response.success
        ? 'settings-status-banner success'
        : 'settings-status-banner error';
      banner.textContent = response.message;
    }
    if (response.success) {
      setFaceIdState('completed');
      if (stepEl) {
        stepEl.textContent = response.message || 'Face matched successfully!';
      }
    } else {
      setFaceIdState('failed');
      if (stepEl) {
        stepEl.textContent = response.message || 'Face recognition failed.';
      }
    }
    showNotification(response.message, response.success ? 'success' : 'error');
  } catch (err) {
    setFaceIdState('failed');
    if (banner) {
      banner.className = 'settings-status-banner error';
      banner.textContent = `Test error: ${err}`;
    }
    if (stepEl) {
      stepEl.textContent = `Test error: ${err}`;
    }
    showNotification(`Test error: ${err}`, 'error');
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML =
        '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg> Test Recognition';
    }
    setTimeout(() => {
      if (progressBox) {
        progressBox.style.display = 'none';
        setFaceIdState('idle');
      }
    }, 4000);
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

  const btnCancelFp = document.getElementById('btn-cancel-fp');
  if (btnCancelFp) {
    btnCancelFp.addEventListener('click', handleCancelFp);
  }

  const btnInstallHowdy = document.getElementById('btn-install-howdy');
  if (btnInstallHowdy) {
    btnInstallHowdy.addEventListener('click', handleInstallHowdy);
  }

  const btnTestFace = document.getElementById('btn-test-face');
  if (btnTestFace) {
    btnTestFace.addEventListener('click', handleTestFace);
  }

  const btnToggleFpLogs = document.getElementById('btn-toggle-fp-logs');
  const fpLogsOutput = document.getElementById('fp-install-logs-output');
  if (btnToggleFpLogs && fpLogsOutput) {
    btnToggleFpLogs.addEventListener('click', () => {
      const isHidden = fpLogsOutput.style.display === 'none';
      fpLogsOutput.style.display = isHidden ? 'block' : 'none';
      btnToggleFpLogs.textContent = isHidden ? 'Hide Logs' : 'Show Logs';
    });
  }

  const btnToggleFaceLogs = document.getElementById('btn-toggle-face-logs');
  const faceLogsOutput = document.getElementById('face-install-logs-output');
  if (btnToggleFaceLogs && faceLogsOutput) {
    btnToggleFaceLogs.addEventListener('click', () => {
      const isHidden = faceLogsOutput.style.display === 'none';
      faceLogsOutput.style.display = isHidden ? 'block' : 'none';
      btnToggleFaceLogs.textContent = isHidden ? 'Hide Logs' : 'Show Logs';
    });
  }

  // Attach Tauri biometrics streaming event listeners
  setupBiometricsEvents();

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
