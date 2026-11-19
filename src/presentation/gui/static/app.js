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

  container.innerHTML = workflows.map(wf => {
    const icon = getWorkflowIcon(wf.name);
    const duration = wf.target_duration_minutes ? `${wf.target_duration_minutes} min` : 'Open';
    const contractClass = wf.contract_type.toLowerCase().includes('unbreakable') ? 'badge-danger' :
                          wf.contract_type.toLowerCase().includes('accountable') ? 'badge-warning' : 'badge-standard';

    const appTags = (wf.allowed_apps || []).slice(0, 4).map(app => `<span class="wf-tag">${escapeHtml(app)}</span>`).join('');
    const domainTags = (wf.allowed_domains || []).slice(0, 3).map(dom => `<span class="wf-tag">${escapeHtml(dom)}</span>`).join('');
    const isCustom = !['wf-study', 'wf-deep-work', 'wf-coding', 'wf-reading'].includes(wf.id);

    return `
      <div class="workflow-card">
        <div class="workflow-card-header">
          <div class="wf-icon-wrap">${icon}</div>
          <div style="display:flex;gap:6px;align-items:center;">
            <span class="wf-badge-contract ${contractClass}">${escapeHtml(wf.contract_type)}</span>
            ${isCustom ? `<button class="btn btn-ghost btn-xs text-muted" onclick="deleteWorkflow('${escapeJs(wf.id)}')" title="Delete custom workflow">✕</button>` : ''}
          </div>
        </div>
        <h3 class="wf-title">${escapeHtml(wf.name)}</h3>
        <div class="wf-duration">Target: ${duration}</div>

        <div class="wf-rules-list">
          <div>
            <div class="wf-rule-group-title">Allowed Applications</div>
            <div class="wf-tags-wrap">${appTags || '<span class="text-muted text-xs">All Apps</span>'}</div>
          </div>
          <div class="mt-2">
            <div class="wf-rule-group-title">Allowed Web Domains</div>
            <div class="wf-tags-wrap">${domainTags || '<span class="text-muted text-xs">All Sites</span>'}</div>
          </div>
        </div>

        <div class="wf-actions-row">
          <button class="btn btn-primary btn-md flex-1" onclick="startWorkflowSession('${escapeJs(wf.id)}')">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
              <polygon points="5 3 19 12 5 21 5 3"/>
            </svg>
            Start Focus Session
          </button>
          <button class="btn btn-secondary btn-md" onclick="forkWorkflowPreset('${escapeJs(wf.id)}')" title="Copy &amp; Customize preset">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
            </svg>
            Fork
          </button>
        </div>
      </div>
    `;
  }).join('');
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
  toast.innerHTML = `<span>${icon}</span><span>${message}</span>`;
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
  container.innerHTML = '<span class="text-muted text-xs py-1">Scanning open desktop windows...</span>';

  try {
    const res = await fetch('/api/system/open-apps');
    if (!res.ok) throw new Error('Could not fetch open applications');
    openRunningApps = await res.json();
    renderOpenAppsChips();
  } catch (err) {
    console.warn('Error loading open apps:', err);
    container.innerHTML = '<span class="text-muted text-xs py-1">No visible windows detected (or running in browser mode).</span>';
  }
}

function renderOpenAppsChips() {
  const container = document.getElementById('open-apps-chips');
  if (!container) return;

  if (!openRunningApps || openRunningApps.length === 0) {
    container.innerHTML = '<span class="text-muted text-xs py-1">No other active desktop windows found.</span>';
    return;
  }

  const filterInput = document.getElementById('filter-running-apps');
  const q = (filterInput ? filterInput.value : '').toLowerCase().trim();

  container.innerHTML = openRunningApps.map(app => {
    const isSelected = selectedApps.has(app.process_name.toLowerCase());
    const shortTitle = app.window_title ? ` (${app.window_title.slice(0, 25)}${app.window_title.length > 25 ? '...' : ''})` : '';
    const textToMatch = `${app.display_name} ${app.window_title} ${app.process_name}`.toLowerCase();
    const isVisible = !q || textToMatch.includes(q);

    return `
      <button type="button" class="chip-preset chip-open-app ${isSelected ? 'selected' : ''}" 
              style="display: ${isVisible ? 'inline-flex' : 'none'};"
              onclick="toggleAppTag('${escapeJs(app.process_name)}', '${escapeJs(app.display_name)}')">
        <span>${isSelected ? '✓' : '+'}</span>
        <strong>${escapeHtml(app.display_name)}</strong>
        <span class="text-muted text-xs">${escapeHtml(shortTitle)}</span>
      </button>
    `;
  }).join('');
}

function filterRunningApps(query) {
  renderOpenAppsChips();
}

function toggleAppTag(processName, displayName) {
  const key = processName.toLowerCase();
  if (selectedApps.has(key)) {
    selectedApps.delete(key);
  } else {
    selectedApps.set(key, displayName || processName);
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
    if (selectedApps.size === 0) {
      appsContainer.innerHTML = '<span class="text-muted text-xs">No apps selected yet (will allow all unless specified)</span>';
    } else {
      let html = '';
      selectedApps.forEach((display, proc) => {
        html += `
          <span class="selected-tag-pill">
            <span>💻 ${escapeHtml(display)}</span>
            <button type="button" class="tag-remove-btn" onclick="toggleAppTag('${escapeJs(proc)}', '${escapeJs(display)}')">&times;</button>
          </span>
        `;
      });
      appsContainer.innerHTML = html;
    }
  }

  // 2. Render Domains Tags
  const domainsContainer = document.getElementById('selected-domains-container');
  if (domainsContainer) {
    if (selectedDomains.size === 0) {
      domainsContainer.innerHTML = '<span class="text-muted text-xs">No websites selected (will allow all unless specified)</span>';
    } else {
      let html = '';
      selectedDomains.forEach((display, dom) => {
        html += `
          <span class="selected-tag-pill">
            <span>🌐 ${escapeHtml(display)}</span>
            <button type="button" class="tag-remove-btn" onclick="toggleDomainTag('${escapeJs(dom)}', '${escapeJs(display)}')">&times;</button>
          </span>
        `;
      });
      domainsContainer.innerHTML = html;
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
    console.error('Poll error:', err);
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
    // Grace expired!
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
    if (snap.recent_activities.length === 0) {
      feedContainer.innerHTML = '<div class="empty-feed-placeholder"><span>Listening for window changes...</span></div>';
    } else {
      feedContainer.innerHTML = snap.recent_activities.map(act => {
        const itemClass = act.is_short_or_reel ? 'feed-short' :
                          act.is_distraction ? 'feed-distraction' : 'feed-productive';
        const icon = getActivityIcon(act.category, act.is_short_or_reel);
        const dur = act.duration_seconds >= 60 ? `${Math.floor(act.duration_seconds / 60)}m ${act.duration_seconds % 60}s` : `${act.duration_seconds}s`;

        return `
          <div class="activity-feed-item ${itemClass}">
            <div class="feed-item-icon">${icon}</div>
            <div class="feed-item-meta">
              <div class="feed-item-top">
                <span class="feed-item-app">${escapeHtml(act.category || act.app_name)}</span>
                <span class="feed-item-duration">${dur}</span>
              </div>
              <div class="feed-item-detail">${escapeHtml(act.detail || act.window_title)}</div>
            </div>
          </div>
        `;
      }).join('');
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

  if (sessions.length === 0) {
    tbody.innerHTML = `<tr><td colspan="8" class="text-center text-muted py-8">No historical sessions recorded yet. Start your first session!</td></tr>`;
    return;
  }

  tbody.innerHTML = sessions.map(s => {
    const dateStr = new Date(s.start_time).toLocaleString();
    const scoreBadge = s.quality_score >= 85 ? 'badge-score-high' :
                       s.quality_score >= 65 ? 'badge-score-med' : 'badge-score-low';

    const exitNote = s.early_exit_reason ? `"${s.early_exit_reason}"` : '—';
    const hasDrifts = s.drifts && s.drifts.length > 0;

    return `
      <tr>
        <td>${dateStr}</td>
        <td><strong>${escapeHtml(s.workflow_name)}</strong></td>
        <td><span class="status-badge">${escapeHtml(s.state)}</span></td>
        <td>${formatNatural(s.total_focus_seconds)}</td>
        <td><span class="badge-score ${scoreBadge}">${s.quality_score.toFixed(1)}%</span></td>
        <td>${s.pull_back_count}</td>
        <td class="text-xs text-muted" style="max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${escapeHtml(exitNote)}</td>
        <td>
          ${hasDrifts ? `<button class="btn btn-secondary btn-sm" onclick="viewDrifts('${escapeJs(s.id)}')">View Drifts (${s.drifts.length})</button>` : '<span class="text-muted text-xs">No Drifts</span>'}
        </td>
      </tr>
    `;
  }).join('');
}

function viewDrifts(sessionId) {
  const session = cachedHistory.find(s => s.id === sessionId);
  if (!session || !session.drifts) return;

  const container = document.getElementById('drift-events-container');
  container.innerHTML = session.drifts.map(d => `
    <div class="active-app-box mb-2">
      <div class="app-avatar">⚠️</div>
      <div class="app-details">
        <div class="app-name-row">
          <span class="app-exe">${escapeHtml(d.target_name)}</span>
          <span class="policy-chip chip-allowed">${escapeHtml(d.resolution)}</span>
        </div>
        <div class="text-xs text-muted">${new Date(d.timestamp).toLocaleTimeString()} — Distraction Duration: ${d.duration_seconds}s</div>
      </div>
    </div>
  `).join('');

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
