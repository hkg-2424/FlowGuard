/* FlowGuard Desktop GUI Application Logic */

// Global State
let currentView = 'focus';
let tourStep = 1;
const TOTAL_TOUR_STEPS = 5;
let livePollInterval = null;
let currentSessionSnapshot = null;
let cachedWorkflows = [];
let cachedHistory = [];
let lastCelebratedPullBackTime = parseInt(localStorage.getItem('flowguard_last_pullback_time') || '0', 10);
let previousPolicyStatus = 'None';

// Web Audio API Synthesized Audio Cues
let audioContext = null;
let soundEnabled = localStorage.getItem('flowguard_sound_enabled') !== 'false';

function initAudio() {
  if (!audioContext) {
    const AudioCtx = window.AudioContext || window.webkitAudioContext;
    if (AudioCtx) {
      audioContext = new AudioCtx();
    }
  }
  if (audioContext && audioContext.state === 'suspended') {
    audioContext.resume();
  }
}

function playGraceChime() {
  if (!soundEnabled) return;
  try {
    initAudio();
    if (!audioContext) return;
    const now = audioContext.currentTime;
    const osc = audioContext.createOscillator();
    const gain = audioContext.createGain();
    osc.type = 'sine';
    osc.frequency.setValueAtTime(329.63, now); // E4
    osc.frequency.setValueAtTime(261.63, now + 0.18); // C4
    gain.gain.setValueAtTime(0.08, now);
    gain.gain.exponentialRampToValueAtTime(0.001, now + 0.45);
    osc.connect(gain);
    gain.connect(audioContext.destination);
    osc.start(now);
    osc.stop(now + 0.45);
  } catch (e) {
    console.debug('Grace audio error:', e);
  }
}

function playInterventionAlarm() {
  if (!soundEnabled) return;
  try {
    initAudio();
    if (!audioContext) return;
    const now = audioContext.currentTime;
    [0, 0.18, 0.36].forEach((offset) => {
      const osc = audioContext.createOscillator();
      const gain = audioContext.createGain();
      osc.type = 'sawtooth';
      osc.frequency.setValueAtTime(440, now + offset);
      osc.frequency.setValueAtTime(349.23, now + offset + 0.09);
      gain.gain.setValueAtTime(0.07, now + offset);
      gain.gain.exponentialRampToValueAtTime(0.001, now + offset + 0.16);
      osc.connect(gain);
      gain.connect(audioContext.destination);
      osc.start(now + offset);
      osc.stop(now + offset + 0.16);
    });
  } catch (e) {
    console.debug('Intervention audio error:', e);
  }
}

function playPullBackChime() {
  if (!soundEnabled) return;
  try {
    initAudio();
    if (!audioContext) return;
    const now = audioContext.currentTime;
    [523.25, 659.25, 783.99].forEach((freq, i) => {
      const osc = audioContext.createOscillator();
      const gain = audioContext.createGain();
      osc.type = 'triangle';
      osc.frequency.setValueAtTime(freq, now + i * 0.08);
      gain.gain.setValueAtTime(0.09, now + i * 0.08);
      gain.gain.exponentialRampToValueAtTime(0.001, now + i * 0.08 + 0.35);
      osc.connect(gain);
      gain.connect(audioContext.destination);
      osc.start(now + i * 0.08);
      osc.stop(now + i * 0.08 + 0.35);
    });
  } catch (e) {
    console.debug('PullBack audio error:', e);
  }
}

function toggleSound() {
  soundEnabled = !soundEnabled;
  localStorage.setItem('flowguard_sound_enabled', soundEnabled ? 'true' : 'false');
  updateSoundButtonUI();
  if (soundEnabled) {
    playPullBackChime();
    showToast('Focus audio cues enabled', 'info');
  } else {
    showToast('Focus audio cues muted', 'info');
  }
}

function updateSoundButtonUI() {
  const icon = document.getElementById('sound-icon');
  const label = document.getElementById('sound-label');
  if (icon) icon.textContent = soundEnabled ? '🔊' : '🔇';
  if (label) label.textContent = soundEnabled ? 'Audio On' : 'Muted';
}

function syncModalBodyLock() {
  const hasOpenModal = !!document.querySelector('.modal-backdrop:not(.hidden)');
  document.body.classList.toggle('modal-open', hasOpenModal);
}

// Initialize on DOM Ready
document.addEventListener('DOMContentLoaded', () => {
  checkFirstLaunchTour();
  loadWorkflows();
  startLivePolling();
  updateSoundButtonUI();

  // Global Escape key listener: dismisses any open modal immediately
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      closeCreateWorkflowModal();
      closeTour();
      closeAccountableModal();
      closeUnbreakableModal();
      closeDriftModal();
      syncModalBodyLock();
    }
  });
});

// Navigation Switching
function switchView(viewName) {
  currentView = viewName;
  document.getElementById('tab-focus').classList.toggle('active', viewName === 'focus');
  document.getElementById('tab-history').classList.toggle('active', viewName === 'history');
  
  document.getElementById('view-focus').classList.toggle('hidden', viewName !== 'focus');
  document.getElementById('view-history').classList.toggle('hidden', viewName !== 'history');

  if (viewName === 'history') {
    loadHistory();
  }
}

// -----------------------------------------------------------------------------
// FEATURE 1: INTERACTIVE ONBOARDING TOUR
// -----------------------------------------------------------------------------
function checkFirstLaunchTour() {
  const hasCompleted = localStorage.getItem('flowguard_onboarding_completed');
  if (!hasCompleted) {
    startTour(false);
  }
}

function startTour(isManual) {
  tourStep = 1;
  updateTourStepUI();
  document.getElementById('modal-tour').classList.remove('hidden');
  syncModalBodyLock();
}

function closeTour() {
  document.getElementById('modal-tour').classList.add('hidden');
  localStorage.setItem('flowguard_onboarding_completed', 'true');
  syncModalBodyLock();
}

function nextTourStep() {
  if (tourStep < TOTAL_TOUR_STEPS) {
    tourStep++;
    updateTourStepUI();
  } else {
    closeTour();
  }
}

function prevTourStep() {
  if (tourStep > 1) {
    tourStep--;
    updateTourStepUI();
  }
}

function goToTourStep(step) {
  tourStep = step;
  updateTourStepUI();
}

function updateTourStepUI() {
  for (let i = 1; i <= TOTAL_TOUR_STEPS; i++) {
    const slide = document.getElementById(`tour-slide-${i}`);
    if (slide) {
      slide.classList.toggle('hidden', i !== tourStep);
    }
  }

  const dots = document.querySelectorAll('.tour-dot');
  dots.forEach((dot, index) => {
    dot.classList.toggle('active', index + 1 === tourStep);
  });

  const prevBtn = document.getElementById('tour-prev-btn');
  const nextBtn = document.getElementById('tour-next-btn');

  if (prevBtn) prevBtn.disabled = (tourStep === 1);
  if (nextBtn) {
    nextBtn.textContent = (tourStep === TOTAL_TOUR_STEPS) ? 'Get Started' : 'Next';
  }
}

// -----------------------------------------------------------------------------
// FEATURE 2: ONE-CLICK WORKFLOW SELECTOR & QUICK CREATION
// -----------------------------------------------------------------------------
async function loadWorkflows() {
  try {
    const res = await fetch('/api/workflows');
    if (!res.ok) throw new Error('Failed to fetch workflows');
    const workflows = await res.json();
    cachedWorkflows = workflows;
    renderWorkflows(workflows);
  } catch (err) {
    console.error('Error loading workflows:', err);
  }
}

function getWorkflowIcon(name) {
  const lower = name.toLowerCase();
  if (lower.includes('study')) return '🎓';
  if (lower.includes('deep')) return '🚀';
  if (lower.includes('code') || lower.includes('dev')) return '💻';
  if (lower.includes('read')) return '📖';
  return '⚡';
}

function renderWorkflows(workflows) {
  const container = document.getElementById('workflows-container');
  if (!container) return;

  container.replaceChildren();

  workflows.forEach(wf => {
    const icon = getWorkflowIcon(wf.name);
    const duration = wf.target_duration_minutes ? `${wf.target_duration_minutes} min` : 'Open';
    const contractClass = wf.contract_type.toLowerCase().includes('unbreakable') ? 'badge-danger' :
                          wf.contract_type.toLowerCase().includes('accountable') ? 'badge-warning' : 'badge-standard';
    const isCustom = !['wf-study', 'wf-deep-work', 'wf-coding', 'wf-reading'].includes(wf.id);

    const card = document.createElement('div');
    card.className = 'workflow-card';

    // Header
    const header = document.createElement('div');
    header.className = 'workflow-card-header';

    const iconWrap = document.createElement('div');
    iconWrap.className = 'wf-icon-wrap';
    iconWrap.textContent = icon;
    header.appendChild(iconWrap);

    const badgeGroup = document.createElement('div');
    badgeGroup.style.display = 'flex';
    badgeGroup.style.gap = '6px';
    badgeGroup.style.alignItems = 'center';

    const contractBadge = document.createElement('span');
    contractBadge.className = `wf-badge-contract ${contractClass}`;
    contractBadge.textContent = wf.contract_type;
    badgeGroup.appendChild(contractBadge);

    if (isCustom) {
      const deleteBtn = document.createElement('button');
      deleteBtn.className = 'btn btn-ghost btn-xs text-muted';
      deleteBtn.title = 'Delete custom workflow';
      deleteBtn.textContent = '✕';
      deleteBtn.addEventListener('click', () => deleteWorkflow(wf.id));
      badgeGroup.appendChild(deleteBtn);
    }
    header.appendChild(badgeGroup);
    card.appendChild(header);

    // Title
    const titleEl = document.createElement('h3');
    titleEl.className = 'wf-title';
    titleEl.textContent = wf.name;
    card.appendChild(titleEl);

    // Duration
    const durEl = document.createElement('div');
    durEl.className = 'wf-duration';
    durEl.textContent = `Target: ${duration}`;
    card.appendChild(durEl);

    // Rules list
    const rulesList = document.createElement('div');
    rulesList.className = 'wf-rules-list';

    // Apps group
    const appsGroup = document.createElement('div');
    const appsTitle = document.createElement('div');
    appsTitle.className = 'wf-rule-group-title';
    appsTitle.textContent = 'Allowed Applications';
    appsGroup.appendChild(appsTitle);

    const appsTagsWrap = document.createElement('div');
    appsTagsWrap.className = 'wf-tags-wrap';
    const allowedApps = wf.allowed_apps || [];
    if (allowedApps.length === 0) {
      const emptySpan = document.createElement('span');
      emptySpan.className = 'text-muted text-xs';
      emptySpan.textContent = 'All Apps';
      appsTagsWrap.appendChild(emptySpan);
    } else {
      allowedApps.slice(0, 4).forEach(app => {
        const tag = document.createElement('span');
        tag.className = 'wf-tag';
        tag.textContent = app;
        appsTagsWrap.appendChild(tag);
      });
    }
    appsGroup.appendChild(appsTagsWrap);
    rulesList.appendChild(appsGroup);

    // Domains group
    const domsGroup = document.createElement('div');
    domsGroup.className = 'mt-2';
    const domsTitle = document.createElement('div');
    domsTitle.className = 'wf-rule-group-title';
    domsTitle.textContent = 'Allowed Web Domains';
    domsGroup.appendChild(domsTitle);

    const domsTagsWrap = document.createElement('div');
    domsTagsWrap.className = 'wf-tags-wrap';
    const allowedDomains = wf.allowed_domains || [];
    if (allowedDomains.length === 0) {
      const emptySpan = document.createElement('span');
      emptySpan.className = 'text-muted text-xs';
      emptySpan.textContent = 'All Sites';
      domsTagsWrap.appendChild(emptySpan);
    } else {
      allowedDomains.slice(0, 3).forEach(dom => {
        const tag = document.createElement('span');
        tag.className = 'wf-tag';
        tag.textContent = dom;
        domsTagsWrap.appendChild(tag);
      });
    }
    domsGroup.appendChild(domsTagsWrap);
    rulesList.appendChild(domsGroup);

    card.appendChild(rulesList);

    // Actions row
    const actionsRow = document.createElement('div');
    actionsRow.className = 'wf-actions-row';

    const startBtn = document.createElement('button');
    startBtn.className = 'btn btn-primary btn-md flex-1';
    const playSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    playSvg.setAttribute('width', '18');
    playSvg.setAttribute('height', '18');
    playSvg.setAttribute('viewBox', '0 0 24 24');
    playSvg.setAttribute('fill', 'currentColor');
    const playPoly = document.createElementNS('http://www.w3.org/2000/svg', 'polygon');
    playPoly.setAttribute('points', '5 3 19 12 5 21 5 3');
    playSvg.appendChild(playPoly);
    startBtn.appendChild(playSvg);
    startBtn.appendChild(document.createTextNode(' Start Focus Session'));
    startBtn.addEventListener('click', () => startWorkflowSession(wf.id));
    actionsRow.appendChild(startBtn);

    const forkBtn = document.createElement('button');
    forkBtn.className = 'btn btn-secondary btn-md';
    forkBtn.title = 'Copy & Customize preset';
    const forkSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    forkSvg.setAttribute('width', '16');
    forkSvg.setAttribute('height', '16');
    forkSvg.setAttribute('viewBox', '0 0 24 24');
    forkSvg.setAttribute('fill', 'none');
    forkSvg.setAttribute('stroke', 'currentColor');
    forkSvg.setAttribute('stroke-width', '2');
    const forkRect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    forkRect.setAttribute('x', '9');
    forkRect.setAttribute('y', '9');
    forkRect.setAttribute('width', '13');
    forkRect.setAttribute('height', '13');
    forkRect.setAttribute('rx', '2');
    forkRect.setAttribute('ry', '2');
    const forkPath = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    forkPath.setAttribute('d', 'M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1');
    forkSvg.appendChild(forkRect);
    forkSvg.appendChild(forkPath);
    forkBtn.appendChild(forkSvg);
    forkBtn.appendChild(document.createTextNode(' Fork'));
    forkBtn.addEventListener('click', () => forkWorkflowPreset(wf.id));
    actionsRow.appendChild(forkBtn);

    card.appendChild(actionsRow);
    container.appendChild(card);
  });
}

// -----------------------------------------------------------------------------
// TOAST NOTIFICATIONS (NON-BLOCKING USER FEEDBACK)
// -----------------------------------------------------------------------------
function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;

  const icon = type === 'success' ? '✓' : type === 'error' ? '⚠️' : 'ℹ️';
  const iconSpan = document.createElement('span');
  iconSpan.textContent = icon;
  const msgSpan = document.createElement('span');
  msgSpan.textContent = message;

  toast.appendChild(iconSpan);
  toast.appendChild(msgSpan);
  container.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    toast.style.transform = 'translateX(40px)';
    toast.style.transition = 'all 0.3s ease';
    setTimeout(() => toast.remove(), 300);
  }, 3500);
}

// -----------------------------------------------------------------------------
// NOVICE-FRIENDLY WORKFLOW BUILDER: LIVE OPEN APPS & 1-CLICK PRESETS
// -----------------------------------------------------------------------------
const selectedApps = new Map();     // key: process_name, value: display_name
const selectedDomains = new Map();  // key: domain, value: display_name
let openRunningApps = [];

function openCreateWorkflowModal() {
  selectedApps.clear();
  selectedDomains.clear();
  document.getElementById('new-wf-name').value = '';
  document.getElementById('new-wf-duration').value = '25';
  document.getElementById('custom-domain-input').value = '';
  const title = document.getElementById('create-modal-title');
  if (title) title.textContent = 'Create Custom Workflow';
  const filterInput = document.getElementById('filter-running-apps');
  if (filterInput) filterInput.value = '';
  renderSelectedTags();
  loadRunningApps();
  document.getElementById('modal-create-workflow').classList.remove('hidden');
  syncModalBodyLock();
}

function forkWorkflowPreset(wfId) {
  const wf = cachedWorkflows.find(w => w.id === wfId);
  if (!wf) return;
  selectedApps.clear();
  selectedDomains.clear();
  document.getElementById('new-wf-name').value = `${wf.name} (Custom)`;
  document.getElementById('new-wf-duration').value = wf.target_duration_minutes || 25;
  document.getElementById('new-wf-contract').value = wf.contract_type.toLowerCase().includes('unbreakable') ? 'unbreakable' :
                                                    wf.contract_type.toLowerCase().includes('accountable') ? 'breakable' : 'standard';
  
  (wf.allowed_apps || []).forEach(app => {
    selectedApps.set(app.toLowerCase(), app);
  });
  (wf.allowed_domains || []).forEach(dom => {
    selectedDomains.set(dom.toLowerCase(), dom);
  });
  const title = document.getElementById('create-modal-title');
  if (title) title.textContent = `Fork & Customize: ${wf.name}`;
  const filterInput = document.getElementById('filter-running-apps');
  if (filterInput) filterInput.value = '';
  renderSelectedTags();
  loadRunningApps();
  document.getElementById('modal-create-workflow').classList.remove('hidden');
  syncModalBodyLock();
}

function closeCreateWorkflowModal() {
  document.getElementById('modal-create-workflow').classList.add('hidden');
  syncModalBodyLock();
}

async function loadRunningApps() {
  const container = document.getElementById('open-apps-chips');
  if (!container) return;
  container.replaceChildren();
  const loading = document.createElement('span');
  loading.className = 'text-muted text-xs py-1';
  loading.textContent = 'Scanning open desktop windows...';
  container.appendChild(loading);

  try {
    const res = await fetch('/api/system/open-apps');
    if (!res.ok) throw new Error('Could not fetch open applications');
    openRunningApps = await res.json();
    renderOpenAppsChips();
  } catch (err) {
    console.warn('Error loading open apps:', err);
    container.replaceChildren();
    const noApps = document.createElement('span');
    noApps.className = 'text-muted text-xs py-1';
    noApps.textContent = 'No visible windows detected (or running in browser mode).';
    container.appendChild(noApps);
  }
}

function cleanBrowserTitle(title) {
  if (!title) return '';
  return title
    .replace(/\s*-\s*(Google Chrome|Microsoft Edge|Brave|Mozilla Firefox|Opera|Vivaldi|Personal\s*-\s*Microsoft Edge).*$/i, '')
    .replace(/\s*-\s*YouTube.*$/i, '')
    .trim();
}

function renderOpenAppsChips() {
  const container = document.getElementById('open-apps-chips');
  if (!container) return;

  container.replaceChildren();

  if (!openRunningApps || openRunningApps.length === 0) {
    const emptySpan = document.createElement('span');
    emptySpan.className = 'text-muted text-xs py-1';
    emptySpan.textContent = 'No other active desktop windows found.';
    container.appendChild(emptySpan);
    return;
  }

  const filterInput = document.getElementById('filter-running-apps');
  const q = (filterInput ? filterInput.value : '').toLowerCase().trim();

  openRunningApps.forEach(app => {
    const isSelected = selectedApps.has(app.process_name.toLowerCase());
    const shortTitle = app.window_title ? ` (${app.window_title.slice(0, 25)}${app.window_title.length > 25 ? '...' : ''})` : '';
    const textToMatch = `${app.display_name} ${app.window_title} ${app.process_name}`.toLowerCase();
    const isVisible = !q || textToMatch.includes(q);

    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `chip-preset chip-open-app ${isSelected ? 'selected' : ''}`;
    btn.style.display = isVisible ? 'inline-flex' : 'none';

    const statusSpan = document.createElement('span');
    statusSpan.textContent = isSelected ? '✓' : '+';
    btn.appendChild(statusSpan);

    const strongEl = document.createElement('strong');
    strongEl.textContent = app.display_name;
    btn.appendChild(strongEl);

    if (shortTitle) {
      const titleSpan = document.createElement('span');
      titleSpan.className = 'text-muted text-xs';
      titleSpan.textContent = shortTitle;
      btn.appendChild(titleSpan);
    }

    btn.addEventListener('click', () => {
      toggleAppTag(app.process_name, app.display_name, app.window_title || '');
    });

    container.appendChild(btn);
  });
}

function filterRunningApps(query) {
  renderOpenAppsChips();
}

function toggleAppTag(processName, displayName, windowTitle) {
  const key = processName.toLowerCase();
  const cleanProc = key.replace(/\.exe$/i, '');
  const isBrowser = ['chrome', 'msedge', 'brave', 'firefox', 'opera', 'vivaldi'].includes(cleanProc);

  if (selectedApps.has(key)) {
    selectedApps.delete(key);
    // If was browser, also remove associated specific page title if present
    if (isBrowser && windowTitle) {
      const cleanTitle = cleanBrowserTitle(windowTitle);
      if (cleanTitle) {
        selectedDomains.delete(cleanTitle.toLowerCase());
      }
    }
  } else {
    selectedApps.set(key, displayName || processName);
    // If selecting an open browser with an active page/lecture, automatically allow that specific course/page
    if (isBrowser && windowTitle) {
      const cleanTitle = cleanBrowserTitle(windowTitle);
      if (cleanTitle && cleanTitle.length >= 3 && !['new tab', 'start page', 'home'].includes(cleanTitle.toLowerCase())) {
        selectedDomains.set(cleanTitle.toLowerCase(), cleanTitle);
        showToast(`Allowed ${displayName || processName} with active page: "${cleanTitle}"`, 'success');
      }
    }
  }
  renderSelectedTags();
  renderOpenAppsChips();
}

function toggleDomainTag(domain, displayName) {
  const key = domain.toLowerCase();
  if (selectedDomains.has(key)) {
    selectedDomains.delete(key);
  } else {
    selectedDomains.set(key, displayName || domain);
  }
  renderSelectedTags();
}

function addCustomDomain() {
  const input = document.getElementById('custom-domain-input');
  if (!input) return;
  const raw = input.value.trim();
  if (!raw) return;

  // Clean domain or extract host from URL
  let domain = raw.toLowerCase();
  try {
    if (domain.startsWith('http://') || domain.startsWith('https://')) {
      const parsed = new URL(domain);
      domain = parsed.hostname;
    } else {
      domain = domain.split('/')[0].split('?')[0].split(':')[0];
    }
    domain = domain.replace(/^www\./, '');
  } catch (_) {
    domain = domain.split('/')[0].split('?')[0].split(':')[0].replace(/^www\./, '');
  }

  if (domain.length < 3) {
    showToast('Please enter a valid website domain or URL', 'error');
    return;
  }

  selectedDomains.set(domain, domain);
  input.value = '';
  renderSelectedTags();
  showToast(`Added allowed site: ${domain}`, 'success');
}

function addSpecificYouTube() {
  const input = document.getElementById('specific-youtube-input');
  if (!input) return;
  const raw = input.value.trim();
  if (!raw) return;

  if (raw.includes('youtube.com/watch') || raw.includes('youtu.be/')) {
    showToast('Please enter the video or course title keywords (e.g. "Complete Web Dev") so FlowGuard can match your browser tab and strictly block Shorts!', 'info');
    return;
  }

  const target = raw.toLowerCase();
  const label = `Lecture: ${raw}`;

  selectedDomains.set(target, label);
  input.value = '';
  renderSelectedTags();
  showToast(`Added allowed lecture/video: ${raw}`, 'success');
}

function clearAllSelectedApps() {
  selectedApps.clear();
  renderSelectedTags();
  renderOpenAppsChips();
}

function clearAllSelectedDomains() {
  selectedDomains.clear();
  renderSelectedTags();
}

function renderSelectedTags() {
  // Update header counts and clear button visibility
  const appsCountLabel = document.getElementById('selected-apps-count-label');
  if (appsCountLabel) {
    appsCountLabel.textContent = `Selected Applications (${selectedApps.size}):`;
  }
  const btnClearApps = document.getElementById('btn-clear-apps');
  if (btnClearApps) {
    btnClearApps.style.display = selectedApps.size > 0 ? 'inline-block' : 'none';
  }

  const domainsCountLabel = document.getElementById('selected-domains-count-label');
  if (domainsCountLabel) {
    domainsCountLabel.textContent = `Selected Websites (${selectedDomains.size}):`;
  }
  const btnClearDomains = document.getElementById('btn-clear-domains');
  if (btnClearDomains) {
    btnClearDomains.style.display = selectedDomains.size > 0 ? 'inline-block' : 'none';
  }

  // 1. Render Apps Tags
  const appsContainer = document.getElementById('selected-apps-container');
  if (appsContainer) {
    appsContainer.replaceChildren();
    if (selectedApps.size === 0) {
      const span = document.createElement('span');
      span.className = 'text-muted text-xs';
      span.textContent = 'No apps selected yet (select at least one app or website above)';
      appsContainer.appendChild(span);
    } else {
      selectedApps.forEach((display, proc) => {
        const pill = document.createElement('span');
        pill.className = 'selected-tag-pill';

        const label = document.createElement('span');
        label.textContent = `💻 ${display}`;
        pill.appendChild(label);

        const removeBtn = document.createElement('button');
        removeBtn.type = 'button';
        removeBtn.className = 'tag-remove-btn';
        removeBtn.textContent = '×';
        removeBtn.addEventListener('click', () => toggleAppTag(proc, display));
        pill.appendChild(removeBtn);

        appsContainer.appendChild(pill);
      });
    }
  }

  // 2. Render Domains Tags
  const domainsContainer = document.getElementById('selected-domains-container');
  if (domainsContainer) {
    domainsContainer.replaceChildren();
    if (selectedDomains.size === 0) {
      const span = document.createElement('span');
      span.className = 'text-muted text-xs';
      span.textContent = 'No websites selected yet';
      domainsContainer.appendChild(span);
    } else {
      selectedDomains.forEach((display, dom) => {
        const pill = document.createElement('span');
        pill.className = 'selected-tag-pill';

        const label = document.createElement('span');
        label.textContent = `🌐 ${display}`;
        pill.appendChild(label);

        const removeBtn = document.createElement('button');
        removeBtn.type = 'button';
        removeBtn.className = 'tag-remove-btn';
        removeBtn.textContent = '×';
        removeBtn.addEventListener('click', () => toggleDomainTag(dom, display));
        pill.appendChild(removeBtn);

        domainsContainer.appendChild(pill);
      });
    }
  }

  // 3. Synchronize hidden form fields
  const appsInput = document.getElementById('new-wf-apps');
  if (appsInput) {
    appsInput.value = Array.from(selectedApps.keys()).join(',');
  }
  const domainsInput = document.getElementById('new-wf-domains');
  if (domainsInput) {
    domainsInput.value = Array.from(selectedDomains.keys()).join(',');
  }

  // 4. Synchronize preset chips visual checked state
  document.querySelectorAll('#preset-apps-cloud [data-app]').forEach(btn => {
    const app = btn.getAttribute('data-app').toLowerCase();
    const isSel = selectedApps.has(app);
    btn.classList.toggle('selected', isSel);
    const statusSpan = btn.querySelector('.chip-status');
    if (statusSpan) statusSpan.textContent = isSel ? '✓' : '+';
  });

  document.querySelectorAll('#preset-domains-cloud [data-domain]').forEach(btn => {
    const dom = btn.getAttribute('data-domain').toLowerCase();
    const isSel = selectedDomains.has(dom);
    btn.classList.toggle('selected', isSel);
    const statusSpan = btn.querySelector('.chip-status');
    if (statusSpan) statusSpan.textContent = isSel ? '✓' : '+';
  });
}

function escapeJs(str) {
  return (str || '')
    .replace(/\\/g, '\\\\')
    .replace(/'/g, "\\'")
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

async function submitCreateWorkflow() {
  const name = document.getElementById('new-wf-name').value.trim();
  const duration = parseInt(document.getElementById('new-wf-duration').value, 10);
  const contract = document.getElementById('new-wf-contract').value;

  if (!name) {
    showToast('Please provide a name for your workflow.', 'error');
    return;
  }

  const allowed_apps = Array.from(selectedApps.keys());
  const allowed_domains = Array.from(selectedDomains.keys());

  // H-18: Prevent creating an immediate silent deny-all workflow
  if (allowed_apps.length === 0 && allowed_domains.length === 0) {
    showToast('Please select at least one allowed application or website. A workflow with no allowed apps or sites will block everything.', 'error');
    return;
  }

  try {
    const res = await fetch('/api/workflows', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        name,
        duration_minutes: duration || 25,
        contract_type: contract,
        min_reason_characters: 10,
        allowed_apps,
        allowed_domains,
      })
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: 'Failed to create workflow' }));
      throw new Error(err.error || res.statusText);
    }
    closeCreateWorkflowModal();
    showToast(`Workflow "${name}" created successfully!`, 'success');
    loadWorkflows();
  } catch (err) {
    showToast('Error creating workflow: ' + err.message, 'error');
  }
}

async function deleteWorkflow(wfId) {
  if (!confirm('Are you sure you want to delete this custom workflow?')) return;
  try {
    const res = await fetch(`/api/workflows/${encodeURIComponent(wfId)}`, { method: 'DELETE' });
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: 'Delete failed' }));
      showToast('Could not delete workflow: ' + (err.error || res.statusText), 'error');
      return;
    }
    showToast('Workflow deleted', 'info');
    loadWorkflows();
  } catch (err) {
    showToast('Error deleting workflow: ' + err.message, 'error');
  }
}

// -----------------------------------------------------------------------------
// FEATURE 3 & 6: LIVE FOCUS SESSION DASHBOARD & REAL-TIME POLLING
// -----------------------------------------------------------------------------
function startLivePolling() {
  pollSession();
  livePollInterval = setInterval(pollSession, 1000);
}

async function pollSession() {
  try {
    const res = await fetch('/api/session/live');
    if (!res.ok) return;
    const snap = await res.json();
    currentSessionSnapshot = snap;
    renderSession(snap);
    updateStatusHeader(snap);
  } catch (err) {
    console.debug('Poll error (server possibly offline):', err);
    const dot = document.getElementById('status-dot');
    if (dot) {
      dot.classList.remove('connected');
      dot.classList.add('disconnected');
    }
    const sourceText = document.getElementById('source-name');
    if (sourceText) {
      sourceText.textContent = 'Disconnected (Server offline)';
    }
  }
}

function updateStatusHeader(snap) {
  const dot = document.getElementById('status-dot');
  const sourceText = document.getElementById('source-name');
  const idleBadge = document.getElementById('idle-badge');

  if (dot) {
    dot.classList.toggle('connected', snap.source_connected);
    dot.classList.toggle('disconnected', !snap.source_connected);
  }

  if (sourceText) {
    sourceText.textContent = snap.source_type || 'Native Windows (Win32)';
  }

  if (idleBadge) {
    const isAfk = snap.idle_state === 'AFK';
    idleBadge.textContent = isAfk ? 'AFK (Timer Paused)' : 'Active';
    idleBadge.classList.toggle('afk', isAfk);
  }
}

function renderSession(snap) {
  // Always update Dashboard Side Panel (Shorts/Reels Radar & Continuous Feed) whether idle or in active session
  renderSidePanel(snap);

  const selectorPanel = document.getElementById('selector-panel');
  const sessionDashboard = document.getElementById('session-dashboard');

  const isSessionActive = snap.state === 'Active' || snap.state === 'Paused';

  if (!isSessionActive) {
    selectorPanel.classList.remove('hidden');
    sessionDashboard.classList.add('hidden');
    previousPolicyStatus = 'None';
    return;
  }

  selectorPanel.classList.add('hidden');
  sessionDashboard.classList.remove('hidden');

  // Workflow Header & Contract Badge
  document.getElementById('session-wf-name').textContent = snap.workflow_name || 'Focus Session';
  const contractBadge = document.getElementById('session-contract-badge');
  contractBadge.textContent = snap.contract_type;

  // Countdown / Elapsed Digits
  const timerDigits = document.getElementById('timer-digits');
  const timerLabel = document.getElementById('timer-label');
  if (snap.remaining_seconds !== null && snap.remaining_seconds !== undefined) {
    timerDigits.textContent = formatTime(snap.remaining_seconds);
    timerLabel.textContent = 'Remaining Time';
  } else {
    timerDigits.textContent = formatTime(snap.elapsed_total_seconds);
    timerLabel.textContent = 'Elapsed Focus';
  }

  // Grace Period Animation & Radial Ring
  const graceRing = document.getElementById('grace-ring');
  const graceLabel = document.getElementById('grace-label');
  const graceSecondsText = document.getElementById('grace-seconds-text');
  const alertBanner = document.getElementById('alert-banner');
  const alertCountdown = document.getElementById('alert-countdown');
  const alertTitle = document.getElementById('alert-title');
  const alertMessage = document.getElementById('alert-message');

  const totalCircumference = 691.15; // 2 * PI * 110

  if (snap.policy_status === 'GracePeriod') {
    document.title = `⚠️ (${snap.grace_remaining_seconds}s Grace) Return to Flow — FlowGuard`;
    // Play gentle audio warning cue upon first entering grace period
    if (previousPolicyStatus !== 'GracePeriod') {
      playGraceChime();
    }

    // 30s Grace Period is ticking!
    graceLabel.classList.remove('hidden');
    graceSecondsText.textContent = `${snap.grace_remaining_seconds}s`;

    alertBanner.classList.remove('hidden');
    alertBanner.classList.remove('intervention');
    alertTitle.textContent = `Distraction Detected: ${snap.violation_target || 'Unauthorized context'}`;
    alertMessage.textContent = 'Return to your allowed workflow before the 30s grace window expires to preserve your 100% Focus Quality Score!';
    alertCountdown.textContent = `${snap.grace_remaining_seconds}s`;

    graceRing.classList.add('in-grace');
    graceRing.classList.remove('in-intervention');

    // Fraction remaining
    const graceFraction = snap.grace_remaining_seconds / (snap.grace_total_seconds || 30);
    graceRing.style.strokeDashoffset = totalCircumference * (1 - graceFraction);

  } else if (snap.policy_status === 'InterventionRequired') {
    // Grace expired! Full active deviation resistance
    document.title = '⛔ FOCUS INTERVENTION REQUIRED! — FlowGuard';
    if (previousPolicyStatus !== 'InterventionRequired') {
      playInterventionAlarm();
      window.focus();
      if ('Notification' in window && Notification.permission === 'granted') {
        try {
          new Notification('⛔ FlowGuard Focus Intervention Required!', {
            body: `You drifted from "${snap.workflow_name || 'Workflow'}" to ${snap.current_app || 'another app'}. Please return to focus!`,
            requireInteraction: true
          });
        } catch (_) {}
      }
    }

    // Show Active Intervention Boundary Screen
    const interventionModal = document.getElementById('modal-active-intervention');
    if (interventionModal && interventionModal.classList.contains('hidden')) {
      interventionModal.classList.remove('hidden');
      syncModalBodyLock();
    }

    const appNameEl = document.getElementById('intervention-app-name');
    if (appNameEl) appNameEl.textContent = snap.current_app || 'Unknown Application';
    const windowTitleEl = document.getElementById('intervention-window-title');
    if (windowTitleEl) windowTitleEl.textContent = snap.current_title || 'Unauthorized Window';
    const wfNameEl = document.getElementById('intervention-wf-name');
    if (wfNameEl) wfNameEl.textContent = snap.workflow_name || 'Active Workflow';

    const btnGiveUp = document.getElementById('btn-intervention-give-up');
    if (btnGiveUp) {
      if (snap.contract_type === 'Unbreakable') {
        btnGiveUp.textContent = '🔒 Early Exit Locked';
        btnGiveUp.disabled = true;
      } else {
        btnGiveUp.textContent = 'End Session';
        btnGiveUp.disabled = false;
      }
    }

    graceLabel.classList.remove('hidden');
    graceSecondsText.textContent = '0s (Distraction Active)';

    alertBanner.classList.remove('hidden');
    alertBanner.classList.add('intervention');
    alertTitle.textContent = `Active Distraction: ${snap.violation_target || 'Distraction'}`;
    alertMessage.textContent = 'Your grace period expired. Switch back to your focus task to resume quality score accumulation.';
    alertCountdown.textContent = 'EXPIRED';

    graceRing.classList.remove('in-grace');
    graceRing.classList.add('in-intervention');
    graceRing.style.strokeDashoffset = totalCircumference;

  } else {
    // In Flow / Allowed Context
    document.title = 'FlowGuard — Focus & Productivity Assistant';
    const interventionModal = document.getElementById('modal-active-intervention');
    if (interventionModal && !interventionModal.classList.contains('hidden')) {
      interventionModal.classList.add('hidden');
      syncModalBodyLock();
    }

    graceLabel.classList.add('hidden');
    alertBanner.classList.add('hidden');

    graceRing.classList.remove('in-grace');
    graceRing.classList.remove('in-intervention');

    // Standard session progress if target duration set
    if (snap.target_duration_seconds) {
      const progressFraction = Math.min(1, snap.elapsed_total_seconds / snap.target_duration_seconds);
      graceRing.style.strokeDashoffset = totalCircumference * progressFraction;
    } else {
      graceRing.style.strokeDashoffset = 0;
    }
  }

  // Celebratory Pull-Back Banner
  if (snap.last_pull_back) {
    const pbTime = new Date(snap.last_pull_back.timestamp).getTime();
    if (pbTime > lastCelebratedPullBackTime) {
      lastCelebratedPullBackTime = pbTime;
      showPullBackCelebration(snap.last_pull_back);
    }
  }

  // Live Foreground App Context
  const liveApp = document.getElementById('live-app-name');
  const liveTitle = document.getElementById('live-window-title');
  const liveDomain = document.getElementById('live-domain-title');
  const policyChip = document.getElementById('policy-chip');
  const livePrompt = document.getElementById('live-prompt-text');
  const promptBox = document.getElementById('live-prompt-box');

  liveApp.textContent = snap.current_app || 'Desktop / System';
  liveTitle.textContent = snap.current_title || 'No active title';
  
  if (snap.current_domain) {
    liveDomain.textContent = snap.current_domain;
    liveDomain.classList.remove('hidden');
  } else {
    liveDomain.classList.add('hidden');
  }

  if (snap.policy_status === 'Allowed') {
    policyChip.className = 'policy-chip chip-allowed';
    policyChip.textContent = 'Allowed Context';
    livePrompt.textContent = 'Excellent! You are working within your authorized workflow rules.';
    promptBox.className = 'live-prompt-box';
  } else if (snap.policy_status === 'GracePeriod') {
    policyChip.className = 'policy-chip chip-grace';
    policyChip.textContent = '30s Grace Active';
    livePrompt.textContent = `Warning: "${snap.violation_target}" is outside your workflow. Close or switch away to self-correct!`;
    promptBox.className = 'live-prompt-box in-grace';
  } else if (snap.policy_status === 'InterventionRequired') {
    policyChip.className = 'policy-chip chip-distraction';
    policyChip.textContent = 'Distraction Episode';
    livePrompt.textContent = 'Intervention required: Return to your authorized focus window now.';
    promptBox.className = 'live-prompt-box in-intervention';
  } else {
    policyChip.className = 'policy-chip';
    policyChip.textContent = snap.policy_status;
    livePrompt.textContent = 'Monitoring focus activity in background...';
    promptBox.className = 'live-prompt-box';
  }

  // Live Metrics
  document.getElementById('stat-focus-time').textContent = formatNatural(snap.focus_seconds);
  document.getElementById('stat-quality-score').textContent = `${snap.quality_score.toFixed(1)}%`;
  document.getElementById('stat-pullbacks').textContent = snap.pull_back_count;
  document.getElementById('stat-grace-distraction').textContent = `${snap.grace_seconds}s / ${snap.distraction_seconds}s`;

  // Pause / Resume Button
  const pauseBtn = document.getElementById('btn-pause-text');
  const pauseButtonEl = document.getElementById('btn-pause-session');
  if (snap.contract_type === 'Unbreakable') {
    if (pauseButtonEl) {
      pauseButtonEl.disabled = true;
      pauseButtonEl.title = 'Unbreakable (Ulysses) sessions cannot be paused';
    }
    pauseBtn.textContent = 'Locked';
  } else {
    if (pauseButtonEl) {
      pauseButtonEl.disabled = false;
      pauseButtonEl.title = 'Pause or resume your focus timer';
    }
    pauseBtn.textContent = snap.state === 'Paused' ? 'Resume' : 'Pause';
  }

  previousPolicyStatus = snap.policy_status;
}

function showPullBackCelebration(pullBack) {
  const banner = document.getElementById('pullback-banner');
  const desc = document.getElementById('pullback-desc');
  desc.textContent = `You pulled back from ${pullBack.target} in ${pullBack.recovery_seconds}s! Focus Quality Score preserved.`;
  banner.classList.remove('hidden');

  // Play subtle web audio notification chime
  playPullBackChime();

  setTimeout(() => {
    banner.classList.add('hidden');
  }, 6000);
}

// -----------------------------------------------------------------------------
// DASHBOARD SIDE PANEL: CONTINUOUS ACTIVITY TRACKING & SHORTS RADAR
// -----------------------------------------------------------------------------
function renderSidePanel(snap) {
  if (!snap) return;

  const summary = snap.activity_summary || {};
  const current = snap.current_activity;

  // 1. Current App & Title
  const appEl = document.getElementById('radar-current-app');
  const titleEl = document.getElementById('radar-current-title');
  const iconEl = document.getElementById('radar-current-icon');
  const catPill = document.getElementById('radar-category-pill');
  const statusBadge = document.getElementById('radar-status-badge');

  if (current) {
    if (appEl) appEl.textContent = current.app_name;
    if (titleEl) titleEl.textContent = current.detail || current.window_title || 'Active';
    if (catPill) catPill.textContent = current.category || 'General';
    if (iconEl) iconEl.textContent = getActivityIcon(current.category, current.is_short_or_reel);
  } else if (snap.current_app) {
    if (appEl) appEl.textContent = snap.current_app;
    if (titleEl) titleEl.textContent = snap.current_title || 'Active';
    if (catPill) catPill.textContent = 'Active';
    if (iconEl) iconEl.textContent = '💻';
  }

  // 2. Status Badge & Shorts Alert
  const shortsBanner = document.getElementById('shorts-alert-banner');
  const shortsPlatform = document.getElementById('shorts-platform-tag');
  const shortsTitle = document.getElementById('shorts-media-title');

  if (summary.is_current_short_or_reel) {
    if (statusBadge) {
      statusBadge.textContent = '⚡ Shorts / Reels Active';
      statusBadge.className = 'badge-status-pill shorts';
    }
    if (shortsBanner) {
      shortsBanner.classList.remove('hidden');
      if (shortsPlatform) shortsPlatform.textContent = `${summary.current_category} Detected!`;
      if (shortsTitle) shortsTitle.textContent = summary.current_detail || 'Shorts / Reels media';
    }
  } else if (summary.is_current_distraction) {
    if (statusBadge) {
      statusBadge.textContent = '⚠️ Distraction';
      statusBadge.className = 'badge-status-pill distraction';
    }
    if (shortsBanner) shortsBanner.classList.add('hidden');
  } else {
    if (statusBadge) {
      statusBadge.textContent = (snap.state === 'Active') ? 'Focus Workflow' : 'Passive Tracking';
      statusBadge.className = 'badge-status-pill';
    }
    if (shortsBanner) shortsBanner.classList.add('hidden');
  }

  // 3. Metrics
  const sideShortsCount = document.getElementById('side-shorts-count');
  const sideShortsTime = document.getElementById('side-shorts-time');
  const sideDistractionTime = document.getElementById('side-distraction-time');
  const sideProductiveTime = document.getElementById('side-productive-time');

  if (sideShortsCount) sideShortsCount.textContent = summary.total_shorts_count || 0;
  if (sideShortsTime) sideShortsTime.textContent = formatDurationSeconds(summary.total_shorts_seconds || 0);
  if (sideDistractionTime) sideDistractionTime.textContent = formatDurationSeconds(summary.total_distraction_seconds || 0);
  if (sideProductiveTime) sideProductiveTime.textContent = formatDurationSeconds(summary.total_productive_seconds || 0);

  // 4. Activity Feed Stream
  const feedContainer = document.getElementById('activity-feed-list');
  if (feedContainer && snap.recent_activities) {
    feedContainer.replaceChildren();
    if (snap.recent_activities.length === 0) {
      const placeholder = document.createElement('div');
      placeholder.className = 'empty-feed-placeholder';
      const span = document.createElement('span');
      span.textContent = 'Listening for window changes...';
      placeholder.appendChild(span);
      feedContainer.appendChild(placeholder);
    } else {
      snap.recent_activities.forEach(act => {
        const itemClass = act.is_short_or_reel ? 'feed-short' :
                          act.is_distraction ? 'feed-distraction' : 'feed-productive';
        const icon = getActivityIcon(act.category, act.is_short_or_reel);
        const dur = act.duration_seconds >= 60 ? `${Math.floor(act.duration_seconds / 60)}m ${act.duration_seconds % 60}s` : `${act.duration_seconds}s`;

        const item = document.createElement('div');
        item.className = `activity-feed-item ${itemClass}`;

        const iconEl = document.createElement('div');
        iconEl.className = 'feed-item-icon';
        iconEl.textContent = icon;
        item.appendChild(iconEl);

        const meta = document.createElement('div');
        meta.className = 'feed-item-meta';

        const top = document.createElement('div');
        top.className = 'feed-item-top';

        const appSpan = document.createElement('span');
        appSpan.className = 'feed-item-app';
        appSpan.textContent = act.category || act.app_name || '';
        top.appendChild(appSpan);

        const durSpan = document.createElement('span');
        durSpan.className = 'feed-item-duration';
        durSpan.textContent = dur;
        top.appendChild(durSpan);

        meta.appendChild(top);

        const detail = document.createElement('div');
        detail.className = 'feed-item-detail';
        detail.textContent = act.detail || act.window_title || '';
        meta.appendChild(detail);

        item.appendChild(meta);
        feedContainer.appendChild(item);
      });
    }
  }
}

function getActivityIcon(category, isShortOrReel) {
  if (isShortOrReel) return '⚡';
  const cat = (category || '').toLowerCase();
  if (cat.includes('short') || cat.includes('reel') || cat.includes('tiktok')) return '⚡';
  if (cat.includes('youtube')) return '🎥';
  if (cat.includes('social') || cat.includes('instagram') || cat.includes('twitter') || cat.includes('reddit')) return '📱';
  if (cat.includes('learn') || cat.includes('udemy') || cat.includes('coursera')) return '🎓';
  if (cat.includes('terminal') || cat.includes('cmd') || cat.includes('powershell')) return '⬛';
  if (cat.includes('code') || cat.includes('vs code') || cat.includes('antigravity') || cat.includes('dev')) return '💻';
  if (cat.includes('notion') || cat.includes('obsidian') || cat.includes('word') || cat.includes('productiv')) return '📝';
  if (cat.includes('web') || cat.includes('browser')) return '🌐';
  return '🖥️';
}

function formatDurationSeconds(sec) {
  if (!sec || sec < 60) return `${sec || 0}s`;
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  if (m < 60) return `${m}m ${s}s`;
  const h = Math.floor(m / 60);
  const remM = m % 60;
  return `${h}h ${remM}m`;
}

function escapeHtml(str) {
  if (!str) return '';
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function playGentleChime() {
  if (!soundEnabled) return;
  try {
    initAudio();
    if (!audioContext) return;
    const now = audioContext.currentTime;
    const osc = audioContext.createOscillator();
    const gain = audioContext.createGain();
    osc.type = 'sine';
    osc.frequency.setValueAtTime(587.33, now); // D5
    osc.frequency.exponentialRampToValueAtTime(880, now + 0.15); // A5
    gain.gain.setValueAtTime(0.12, now);
    gain.gain.exponentialRampToValueAtTime(0.001, now + 0.35);
    osc.connect(gain);
    gain.connect(audioContext.destination);
    osc.start(now);
    osc.stop(now + 0.35);
  } catch (e) {
    // Ignore audio autoplay restrictions
  }
}

// -----------------------------------------------------------------------------
// SESSION CONTROL ACTIONS
// -----------------------------------------------------------------------------
async function startWorkflowSession(workflowId) {
  initAudio();
  if ('Notification' in window && Notification.permission === 'default') {
    Notification.requestPermission().catch(() => {});
  }
  try {
    const res = await fetch('/api/session/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ workflow_id: workflowId })
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      showToast('Could not start session: ' + (err.error || res.statusText), 'error');
      return;
    }

    showToast('Focus session started! Stay in your flow.', 'success');
    pollSession();
  } catch (err) {
    showToast('Network error starting session: ' + err.message, 'error');
  }
}

async function togglePauseResume() {
  if (!currentSessionSnapshot) return;
  if (currentSessionSnapshot.contract_type === 'Unbreakable') {
    showToast('Unbreakable (Ulysses) sessions cannot be paused.', 'error');
    return;
  }
  initAudio();
  const isPaused = currentSessionSnapshot.state === 'Paused';
  const endpoint = isPaused ? '/api/session/resume' : '/api/session/pause';

  try {
    const res = await fetch(endpoint, { method: 'POST' });
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: 'Action failed' }));
      showToast('Pause/Resume error: ' + (err.error || res.statusText), 'error');
      return;
    }
    pollSession();
  } catch (err) {
    showToast('Network error: ' + err.message, 'error');
  }
}

// -----------------------------------------------------------------------------
// FEATURE 4: ACCOUNTABLE EXIT MODAL & REFLECTION HANDLING
// -----------------------------------------------------------------------------
function handleEndSessionClick() {
  if (!currentSessionSnapshot) return;

  const contract = currentSessionSnapshot.contract_type;

  if (contract === 'Unbreakable') {
    // Ulysses lock: forbid cancellation
    document.getElementById('modal-unbreakable-warning').classList.remove('hidden');
    syncModalBodyLock();
    return;
  }

  if (contract === 'AccountableBreakable') {
    // Open accountable exit reflection modal
    const minChars = currentSessionSnapshot.min_reason_characters || 10;
    document.getElementById('reason-min-count').textContent = minChars;
    document.getElementById('exit-reason-input').value = '';
    updateReasonCharCount();
    document.getElementById('modal-accountable-exit').classList.remove('hidden');
    syncModalBodyLock();
    return;
  }

  // Standard contract: confirm directly
  if (confirm('Are you sure you want to end your focus session?')) {
    stopSessionDirect(null);
  }
}

function updateReasonCharCount() {
  const input = document.getElementById('exit-reason-input');
  const countSpan = document.getElementById('reason-char-count');
  const btn = document.getElementById('btn-submit-early-exit');

  const minChars = currentSessionSnapshot?.min_reason_characters || 10;
  const len = input.value.trim().length;

  countSpan.textContent = len;
  btn.disabled = (len < minChars);
}

function closeAccountableModal() {
  document.getElementById('modal-accountable-exit').classList.add('hidden');
  syncModalBodyLock();
}

function closeUnbreakableModal() {
  document.getElementById('modal-unbreakable-warning').classList.add('hidden');
  syncModalBodyLock();
}

function dismissInterventionModal() {
  const modal = document.getElementById('modal-active-intervention');
  if (modal) modal.classList.add('hidden');
  syncModalBodyLock();
  showToast('Returning to allowed focus tools...', 'info');
}

function handleInterventionGiveUp() {
  dismissInterventionModal();
  handleEndSessionClick();
}

async function submitEarlyExit() {
  const reason = document.getElementById('exit-reason-input').value.trim();
  closeAccountableModal();
  stopSessionDirect(reason);
}

async function stopSessionDirect(reason) {
  try {
    const res = await fetch('/api/session/stop', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ reason })
    });

    if (!res.ok) {
      const err = await res.json();
      showToast('Error ending session: ' + (err.error || res.statusText), 'error');
      return;
    }

    pollSession();
    loadHistory();
  } catch (err) {
    showToast('Error stopping session: ' + err.message, 'error');
  }
}

// -----------------------------------------------------------------------------
// FEATURE 5: HISTORY & ANALYTICS VIEW
// -----------------------------------------------------------------------------
async function loadHistory() {
  try {
    const res = await fetch('/api/history');
    if (!res.ok) return;
    const history = await res.json();
    cachedHistory = history;
    renderHistory(history);
  } catch (err) {
    console.error('History fetch error:', err);
  }
}

function renderHistory(sessions) {
  // Aggregate stats
  let totalFocusSecs = 0;
  let totalCompleted = 0;
  let totalScoreSum = 0;
  let totalPullBacks = 0;

  sessions.forEach(s => {
    totalFocusSecs += s.total_focus_seconds;
    if (s.state === 'Completed') totalCompleted++;
    totalScoreSum += s.quality_score;
    totalPullBacks += s.pull_back_count;
  });

  const avgScore = sessions.length > 0 ? (totalScoreSum / sessions.length) : 100;

  document.getElementById('hist-total-focus').textContent = formatHoursMins(totalFocusSecs);
  document.getElementById('hist-total-sessions').textContent = totalCompleted;
  document.getElementById('hist-avg-score').textContent = `${avgScore.toFixed(1)}%`;
  document.getElementById('hist-total-pullbacks').textContent = totalPullBacks;

  // Table
  const tbody = document.getElementById('history-table-body');
  if (!tbody) return;

  tbody.replaceChildren();

  if (sessions.length === 0) {
    const tr = document.createElement('tr');
    const td = document.createElement('td');
    td.colSpan = 8;
    td.className = 'text-center text-muted py-8';
    td.textContent = 'No historical sessions recorded yet. Start your first session!';
    tr.appendChild(td);
    tbody.appendChild(tr);
    return;
  }

  sessions.forEach(s => {
    const dateStr = new Date(s.start_time).toLocaleString();
    const scoreBadge = s.quality_score >= 85 ? 'badge-score-high' :
                       s.quality_score >= 65 ? 'badge-score-med' : 'badge-score-low';
    const exitNote = s.early_exit_reason ? `"${s.early_exit_reason}"` : '—';
    const hasDrifts = s.drifts && s.drifts.length > 0;

    const tr = document.createElement('tr');

    const tdDate = document.createElement('td');
    tdDate.textContent = dateStr;
    tr.appendChild(tdDate);

    const tdName = document.createElement('td');
    const strongName = document.createElement('strong');
    strongName.textContent = s.workflow_name;
    tdName.appendChild(strongName);
    tr.appendChild(tdName);

    const tdState = document.createElement('td');
    const stateBadge = document.createElement('span');
    stateBadge.className = 'status-badge';
    stateBadge.textContent = s.state;
    tdState.appendChild(stateBadge);
    tr.appendChild(tdState);

    const tdFocus = document.createElement('td');
    tdFocus.textContent = formatNatural(s.total_focus_seconds);
    tr.appendChild(tdFocus);

    const tdScore = document.createElement('td');
    const scoreSpan = document.createElement('span');
    scoreSpan.className = `badge-score ${scoreBadge}`;
    scoreSpan.textContent = `${s.quality_score.toFixed(1)}%`;
    tdScore.appendChild(scoreSpan);
    tr.appendChild(tdScore);

    const tdPullbacks = document.createElement('td');
    tdPullbacks.textContent = s.pull_back_count;
    tr.appendChild(tdPullbacks);

    const tdReason = document.createElement('td');
    tdReason.className = 'text-xs text-muted';
    tdReason.style.maxWidth = '200px';
    tdReason.style.overflow = 'hidden';
    tdReason.style.textOverflow = 'ellipsis';
    tdReason.style.whiteSpace = 'nowrap';
    tdReason.textContent = exitNote;
    tr.appendChild(tdReason);

    const tdAction = document.createElement('td');
    if (hasDrifts) {
      const btn = document.createElement('button');
      btn.className = 'btn btn-secondary btn-sm';
      btn.textContent = `View Drifts (${s.drifts.length})`;
      btn.addEventListener('click', () => viewDrifts(s.id));
      tdAction.appendChild(btn);
    } else {
      const noDriftsSpan = document.createElement('span');
      noDriftsSpan.className = 'text-muted text-xs';
      noDriftsSpan.textContent = 'No Drifts';
      tdAction.appendChild(noDriftsSpan);
    }
    tr.appendChild(tdAction);

    tbody.appendChild(tr);
  });
}

function viewDrifts(sessionId) {
  const session = cachedHistory.find(s => s.id === sessionId);
  if (!session || !session.drifts) return;

  const container = document.getElementById('drift-events-container');
  container.replaceChildren();

  session.drifts.forEach(d => {
    const box = document.createElement('div');
    box.className = 'active-app-box mb-2';

    const avatar = document.createElement('div');
    avatar.className = 'app-avatar';
    avatar.textContent = '⚠️';
    box.appendChild(avatar);

    const details = document.createElement('div');
    details.className = 'app-details';

    const nameRow = document.createElement('div');
    nameRow.className = 'app-name-row';

    const exeSpan = document.createElement('span');
    exeSpan.className = 'app-exe';
    exeSpan.textContent = d.target_name;
    nameRow.appendChild(exeSpan);

    const resChip = document.createElement('span');
    resChip.className = 'policy-chip chip-allowed';
    resChip.textContent = d.resolution;
    nameRow.appendChild(resChip);

    details.appendChild(nameRow);

    const metaDiv = document.createElement('div');
    metaDiv.className = 'text-xs text-muted';
    metaDiv.textContent = `${new Date(d.timestamp).toLocaleTimeString()} — Distraction Duration: ${d.duration_seconds}s`;
    details.appendChild(metaDiv);

    box.appendChild(details);
    container.appendChild(box);
  });

  document.getElementById('modal-drift-events').classList.remove('hidden');
  syncModalBodyLock();
}

function closeDriftModal() {
  document.getElementById('modal-drift-events').classList.add('hidden');
  syncModalBodyLock();
}

// -----------------------------------------------------------------------------
// HELPER FORMATTERS
// -----------------------------------------------------------------------------
function formatTime(totalSeconds) {
  const mins = Math.floor(totalSeconds / 60);
  const secs = Math.floor(totalSeconds % 60);
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
}

function formatNatural(totalSeconds) {
  const mins = Math.floor(totalSeconds / 60);
  const secs = Math.floor(totalSeconds % 60);
  if (mins === 0) return `${secs}s`;
  return `${mins}m ${secs.toString().padStart(2, '0')}s`;
}

function formatHoursMins(totalSeconds) {
  const hours = Math.floor(totalSeconds / 3600);
  const mins = Math.floor((totalSeconds % 3600) / 60);
  return `${hours}h ${mins}m`;
}
