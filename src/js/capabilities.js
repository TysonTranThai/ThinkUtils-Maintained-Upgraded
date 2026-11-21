// What this machine can actually do, as reported by the backend.
//
// Every view used to assume the Linux hardware paths existed. On Windows most
// of them do not, so the UI has to ask before it paints a control that cannot
// work. The backend owns the detection; this module only caches the answer and
// applies it to the DOM.
//
// When the capability query fails the UI deliberately fails open: the previous
// behaviour (show the control, report the error after the click) is better than
// hiding every feature because one command failed.

const { invoke } = window.__TAURI__.core;

const STATUS_LABELS = {
  supported: 'Available',
  partial: 'Limited',
  monitoring_only: 'Read-only',
  admin_required: 'Setup needed',
  unsupported: 'Unavailable'
};

const NOTICE_STATUSES = new Set(['partial', 'monitoring_only', 'admin_required']);

let capabilities = null;
let featureMap = new Map();
let loadPromise = null;

export async function loadCapabilities() {
  if (loadPromise) {
    return loadPromise;
  }

  loadPromise = (async () => {
    try {
      const response = await invoke('get_capabilities');
      if (response.success && response.data) {
        capabilities = response.data;
        featureMap = new Map(response.data.features.map((feature) => [feature.id, feature]));
        console.log(
          `[Capabilities] ${capabilities.platform_name}: ${capabilities.features.length} features`
        );
      } else {
        console.warn('[Capabilities] Backend returned no capability report:', response.error);
      }
    } catch (error) {
      console.error('[Capabilities] Could not load capabilities:', error);
    }
    return capabilities;
  })();

  return loadPromise;
}

export function getCapabilities() {
  return capabilities;
}

export function getCapability(id) {
  return featureMap.get(id) ?? null;
}

export function capabilityStatus(id) {
  // Fail open: an unknown feature is treated as usable, not hidden.
  return featureMap.get(id)?.status ?? 'supported';
}

export function isAvailable(id) {
  return capabilityStatus(id) !== 'unsupported';
}

export function isSupported(id) {
  return capabilityStatus(id) === 'supported';
}

export function isWindows() {
  return capabilities?.platform === 'windows';
}

function createNotice(capability) {
  const notice = document.createElement('div');
  notice.className = `capability-notice capability-${capability.status}`;
  notice.setAttribute('role', 'status');

  const title = document.createElement('strong');
  title.textContent = `${capability.title} — ${STATUS_LABELS[capability.status] ?? capability.status}`;

  const detail = document.createElement('span');
  detail.textContent = capability.detail;

  notice.append(title, detail);
  return notice;
}

function insertNoticeAfter(element, capability) {
  // The card may already have a notice from a previous application (for
  // example if capabilities are re-applied after an import). Replace it.
  const existing = element.nextElementSibling;
  if (existing?.classList?.contains('capability-notice')) {
    existing.remove();
  }

  const notice = createNotice(capability);
  notice.dataset.capabilityNoticeFor = capability.id;
  element.parentNode?.insertBefore(notice, element.nextSibling);
}

function hideElement(element) {
  element.style.display = 'none';
  element.setAttribute('aria-hidden', 'true');
}

/**
 * Apply capability gating to the sidebar and static markup.
 *
 * Markup opts in with three attributes:
 *   data-capability="<id>"            hide the element when unsupported and
 *                                     leave an explanation in its place.
 *   data-capability-controls="<id>"   hide the element unless the feature is
 *                                     fully supported (read-only modes keep the
 *                                     page but lose the controls).
 *   data-capability-notice="<id>"     insert a status notice at the top of the
 *                                     element for partial/read-only states.
 */
export function applyCapabilityUi() {
  applyNavigationCapabilities();

  document.querySelectorAll('[data-capability]').forEach((element) => {
    const capability = getCapability(element.dataset.capability);
    if (!capability || capability.status !== 'unsupported') {
      return;
    }
    hideElement(element);
    if (!element.hasAttribute('data-capability-silent')) {
      insertNoticeAfter(element, capability);
    }
  });

  document.querySelectorAll('[data-capability-controls]').forEach((element) => {
    const capability = getCapability(element.dataset.capabilityControls);
    if (!capability || capability.status === 'supported') {
      return;
    }
    hideElement(element);
  });

  document.querySelectorAll('[data-capability-notice]').forEach((element) => {
    const capability = getCapability(element.dataset.capabilityNotice);
    if (!capability || !NOTICE_STATUSES.has(capability.status)) {
      return;
    }
    if (
      element.querySelector(`.capability-notice[data-capability-notice-for="${capability.id}"]`)
    ) {
      return;
    }
    const notice = createNotice(capability);
    notice.dataset.capabilityNoticeFor = capability.id;
    element.prepend(notice);
  });
}

export function applyNavigationCapabilities() {
  document.querySelectorAll('.menu-item[data-feature]').forEach((item) => {
    const capability = getCapability(item.dataset.feature);
    if (!capability) {
      return;
    }

    // Remove any badge from an earlier application.
    item.querySelector('.menu-status-badge')?.remove();
    item.classList.remove('disabled', 'limited');
    item.removeAttribute('aria-disabled');
    item.removeAttribute('title');

    if (capability.status === 'unsupported') {
      item.classList.add('disabled');
      item.setAttribute('aria-disabled', 'true');
      item.title = capability.detail;
      addBadge(item, STATUS_LABELS.unsupported);
    } else if (capability.status !== 'supported') {
      item.classList.add('limited');
      item.title = capability.detail;
      addBadge(item, STATUS_LABELS[capability.status] ?? capability.status);
    }
  });
}

function addBadge(item, label) {
  const badge = document.createElement('span');
  badge.className = 'menu-status-badge';
  badge.textContent = label;
  item.appendChild(badge);
}

/**
 * Insert a platform-specific explanation into a view. Used where the status is
 * not a capability of its own (Windows Hello, for example).
 */
export function showCapabilityNote(container, title, detail) {
  if (!container) {
    return;
  }
  if (container.querySelector(':scope > .capability-notice.platform-note')) {
    return;
  }

  const notice = document.createElement('div');
  notice.className = 'capability-notice platform-note';
  notice.setAttribute('role', 'status');

  const heading = document.createElement('strong');
  heading.textContent = title;

  const text = document.createElement('span');
  text.textContent = detail;

  notice.append(heading, text);
  container.prepend(notice);
}
