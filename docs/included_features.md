# FlowGuard — Included Features

This document details all features currently implemented and active in the FlowGuard codebase.

---

## 1. Native Windows Win32 Zero-Telemetry Observer
- **Zero Daemon Overhead**: Unlike other focus trackers, FlowGuard does **not** require running ActivityWatch, Python daemons, or background node processes. Everything is 100% self-contained in a single, lightweight binary.
- **Hardware AFK Detection**: Utilizes native `GetLastInputInfo` to measure user physical idle time. When the user steps away from their keyboard or mouse, the focus and grace timers freeze immediately so no focus minutes or grace runway are penalized.
- **Instant Event Hooking**: Uses native Windows `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`) with an automatic polling fallback for ultra-low CPU (<0.2%) foreground window tracking.

---

## 2. The 30-Second Grace Window & Persistent Runway
- **Human-Centric Attention Runway**: When an unallowed application or distracting website is brought to the foreground, FlowGuard does not harshly interrupt or punish the user immediately. Instead, it activates an animated 30-second Grace Period.
- **Persistent Anti-Hopping Countdown**: If the user hops between different unauthorized apps or websites (e.g. from Discord to Steam to YouTube Shorts), the 30-second grace timer **does not reset**. It preserves the original countdown timestamp until the user actually returns to an authorized application or website defined in their workflow.

---

## 3. Pull-Back Resilience & Quality Scoring
- **Celebration of Recovery**: Focus is the ability to recover from drift. When a user navigates back to their authorized workflow within the 30-second window, FlowGuard records a **Pull-Back (Self-Correction)** event.
- **100% Focus Quality Score Preservation**: Recovering within the grace window protects the session's 100% Focus Quality Score, fostering positive reinforcement rather than shame.
- **Real-Time Visual & Audio Recognition**: Displays an uplifting banner celebration in the GUI and plays a harmonious major-chord chime upon recovery.

---

## 4. Tri-Tier Commitment Contracts
- **Standard (Flexible)**: Free-flow sessions that can be paused or ended anytime without friction.
- **Accountable Breakable**: Requires the user to enter a thoughtful typed reflection (minimum 10 characters) explaining why they are ending their focus block early. This stops impulsive, unconscious tab/app quitting.
- **Unbreakable (Ulysses Contract)**: Hardened focus lock. For high-stakes deep work sprints where early termination is strictly disabled until the session timer expires.

---

## 5. 24/7 Passive Activity Tracking & Shorts/Reels Radar
- **Always-On Passive Tracking**: Even when no focus session is active, FlowGuard's background observer continuously categorizes foreground activity.
- **Real-Time YouTube Shorts & Instagram Reels Detection**: Automatically detects when the active window is playing YouTube Shorts, Instagram Reels, or TikTok videos. Captures the specific video/media title, platform badge, and dwell duration.
- **Live Dashboard Side Panel**:
  - **Live Activity Radar**: Real-time card showing active foreground application, window title, and category badge.
  - **Shorts/Reels Alert Pill**: Eye-catching alert badge that lights up when a Short or Reel is active.
  - **Daily Distraction Metrics**: Displays count of Shorts/Reels watched today, total Shorts watch time, total distraction time, and productive time.
  - **Activity & Media Stream**: Clean, scrollable chronological activity feed with dwell timestamps.

---

## 6. Novice-Friendly 1-Click Workflow Builder
- **Zero Technical Jargon**: Users never need to know `.exe` filenames, file paths, or regex patterns.
- **Live Desktop Window Scanner**: Enumerates all currently running desktop windows with friendly human-readable titles (e.g., "Visual Studio Code", "Brave Browser (Udemy Course)", "Terminal").
- **1-Click Preset Cloud**: Instant toggle buttons for top work tools (VS Code, Antigravity, Terminal, Obsidian, Notion, Word, PDF Reader) and learning platforms (Udemy, Coursera, YouTube, ChatGPT, Claude, GitHub, Docs.rs, Wikipedia).
- **Interactive Checkmark Badges**: Buttons visually indicate selection state with green checkmarks and highlighted borders.
- **Smart Top Summary**: The selected applications and websites summary box is pinned at the top of the modal for instant visual confirmation.
- **Intelligent URL Extraction**: Users can paste any full link (e.g., `https://www.udemy.com/course/rust-deep-dive/...`), and FlowGuard automatically strips query strings and extracts the clean domain (`udemy.com`).

---

## 7. Fully Accessible Modal Controls & Seamless Scrolling
- **No Trap / Locked Windows**: Modals never clip off-screen. Modal bodies feature responsive `max-height: 88vh` with custom scrollbars.
- **Sticky Actions**: Headers and footers stay fixed in view with a clear `Cancel & Close (Esc)` button.
- **Multi-Mechanism Exit**: Any open modal can be dismissed immediately via:
  1. The keyboard `Escape` key.
  2. Clicking anywhere on the darkened backdrop outside the modal card.
  3. Clicking the top-right `✕` close button.
  4. Clicking the footer `Cancel` button.

---

## 8. Gentle Web Audio API Sound Cues
- **Zero Audio File Assets**: Synthesizes soft, pleasant sound effects directly in code using the browser Web Audio API oscillator.
- **Grace Period Audio Cue**: Dual-tone subtle warning tone alerting the user when their focus has drifted even if FlowGuard is minimized or in the background.
- **Pull-Back Victory Chime**: Harmonious 3-tone ascending chord congratulating the user for catching themselves and returning to flow.
- **Sound Toggle Control**: Quick mute/unmute button with preference persisted in local storage.

---

## 9. 1-Click Workflow Preset Duplicator / Customizer
- Each workflow card in the Focus Hub provides a **Customize & Fork** button.
- Novice users can duplicate existing presets like "Study" or "Deep Work" and tailor allowed tools in seconds without configuring from scratch.

---

## 10. Local SQLite Session History & Analytics Engine
- Complete, local-only session history stored in SQLite (`flowguard.db`).
- Displays total focus time, sessions completed, average quality scores, and logged pull-backs.
- Interactive drift event viewer showing exact timestamps, unauthorized window targets, and recovery durations.
