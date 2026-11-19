/* FlowGuard Desktop GUI Application Logic */

// Global State
let currentView = 'focus';
let tourStep = 1;
const TOTAL_TOUR_STEPS = 5;
let livePollInterval = null;
let currentSessionSnapshot = null;
let cachedWorkflows = [];
let cachedHistory = [];
let lastCelebratedPullBackTime = 0;

// Initialize on DOM Ready
document.addEventListener('DOMContentLoaded', () => {
  checkFirstLaunchTour();
  loadWorkflows();
  startLivePolling();
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
}

function closeTour() {
  document.getElementById('modal-tour').classList.add('hidden');
  localStorage.setItem('flowguard_onboarding_completed', 'true');
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

    const appTags = wf.allowed_apps.slice(0, 4).map(app => `<span class="wf-tag">${app}</span>`).join('');
    const domainTags = wf.allowed_domains.slice(0, 3).map(dom => `<span class="wf-tag">${dom}</span>`).join('');

    return `
      <div class="workflow-card">
        <div class="workflow-card-header">
          <div class="wf-icon-wrap">${icon}</div>
          <span class="wf-badge-contract ${contractClass}">${wf.contract_type}</span>
        </div>
        <h3 class="wf-title">${wf.name}</h3>
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

        <button class="btn btn-primary btn-md w-full" onclick="startWorkflowSession('${wf.id}')">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
            <polygon points="5 3 19 12 5 21 5 3"/>
          </svg>
          Start Focus Session
        </button>
      </div>
    `;
  }).join('');
}

function openCreateWorkflowModal() {
  document.getElementById('modal-create-workflow').classList.remove('hidden');
}

function closeCreateWorkflowModal() {
  document.getElementById('modal-create-workflow').classList.add('hidden');
}

async function submitCreateWorkflow() {
  const name = document.getElementById('new-wf-name').value.trim();
  const duration = parseInt(document.getElementById('new-wf-duration').value, 10);
  const contract = document.getElementById('new-wf-contract').value;
  const appsRaw = document.getElementById('new-wf-apps').value.trim();
  const domainsRaw = document.getElementById('new-wf-domains').value.trim();

  if (!name) {
    alert('Please provide a name for your workflow.');
    return;
  }

  const allowed_apps = appsRaw ? appsRaw.split(',').map(s => s.trim().toLowerCase()).filter(Boolean) : [];
  const allowed_domains = domainsRaw ? domainsRaw.split(',').map(s => s.trim().toLowerCase()).filter(Boolean) : [];

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

    if (!res.ok) throw new Error('Failed to create workflow');
    closeCreateWorkflowModal();
    loadWorkflows();
  } catch (err) {
    alert('Error creating workflow: ' + err.message);
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
  const selectorPanel = document.getElementById('selector-panel');
  const sessionDashboard = document.getElementById('session-dashboard');

  const isSessionActive = snap.state === 'Active' || snap.state === 'Paused';

  if (!isSessionActive) {
    selectorPanel.classList.remove('hidden');
    sessionDashboard.classList.add('hidden');
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
    promptBox.className = 'live-prompt-box in-grace';
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
  pauseBtn.textContent = snap.state === 'Paused' ? 'Resume' : 'Pause';
}

function showPullBackCelebration(pullBack) {
  const banner = document.getElementById('pullback-banner');
  const desc = document.getElementById('pullback-desc');
  desc.textContent = `You pulled back from ${pullBack.target} in ${pullBack.recovery_seconds}s! Focus Quality Score preserved.`;
  banner.classList.remove('hidden');

  // Play subtle web audio notification chime
  playGentleChime();

  setTimeout(() => {
    banner.classList.add('hidden');
  }, 6000);
}

function playGentleChime() {
  try {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = 'sine';
    osc.frequency.setValueAtTime(587.33, ctx.currentTime); // D5
    osc.frequency.exponentialRampToValueAtTime(880, ctx.currentTime + 0.15); // A5
    gain.gain.setValueAtTime(0.12, ctx.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.35);
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + 0.35);
  } catch (e) {
    // Ignore audio autoplay restrictions
  }
}

// -----------------------------------------------------------------------------
// SESSION CONTROL ACTIONS
// -----------------------------------------------------------------------------
async function startWorkflowSession(workflowId) {
  try {
    const res = await fetch('/api/session/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ workflow_id: workflowId })
    });

    if (!res.ok) {
      const err = await res.json();
      alert('Could not start session: ' + (err.error || res.statusText));
      return;
    }

    pollSession();
  } catch (err) {
    alert('Error starting session: ' + err.message);
  }
}

async function togglePauseResume() {
  if (!currentSessionSnapshot) return;
  const isPaused = currentSessionSnapshot.state === 'Paused';
  const endpoint = isPaused ? '/api/session/resume' : '/api/session/pause';

  try {
    const res = await fetch(endpoint, { method: 'POST' });
    if (!res.ok) throw new Error('Action failed');
    pollSession();
  } catch (err) {
    console.error(err);
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
    return;
  }

  if (contract === 'AccountableBreakable') {
    // Open accountable exit reflection modal
    const minChars = currentSessionSnapshot.min_reason_characters || 10;
    document.getElementById('reason-min-count').textContent = minChars;
    document.getElementById('exit-reason-input').value = '';
    updateReasonCharCount();
    document.getElementById('modal-accountable-exit').classList.remove('hidden');
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
}

function closeUnbreakableModal() {
  document.getElementById('modal-unbreakable-warning').classList.add('hidden');
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
      alert('Error ending session: ' + (err.error || res.statusText));
      return;
    }

    pollSession();
    loadHistory();
  } catch (err) {
    alert('Error stopping session: ' + err.message);
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
        <td><strong>${s.workflow_name}</strong></td>
        <td><span class="status-badge">${s.state}</span></td>
        <td>${formatNatural(s.total_focus_seconds)}</td>
        <td><span class="badge-score ${scoreBadge}">${s.quality_score.toFixed(1)}%</span></td>
        <td>${s.pull_back_count}</td>
        <td class="text-xs text-muted" style="max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${exitNote}</td>
        <td>
          ${hasDrifts ? `<button class="btn btn-secondary btn-sm" onclick="viewDrifts('${s.id}')">View Drifts (${s.drifts.length})</button>` : '<span class="text-muted text-xs">No Drifts</span>'}
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
          <span class="app-exe">${d.target_name}</span>
          <span class="policy-chip chip-allowed">${d.resolution}</span>
        </div>
        <div class="text-xs text-muted">${new Date(d.timestamp).toLocaleTimeString()} — Distraction Duration: ${d.duration_seconds}s</div>
      </div>
    </div>
  `).join('');

  document.getElementById('modal-drift-events').classList.remove('hidden');
}

function closeDriftModal() {
  document.getElementById('modal-drift-events').classList.add('hidden');
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
