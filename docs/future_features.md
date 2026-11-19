# FlowGuard — Future Features Roadmap

This document outlines high-value future features and enhancements planned for subsequent releases of FlowGuard, focused on novice accessibility, mindful productivity, and desktop integration.

---

## 1. Smart Application Auto-Launcher & Workspace Restorer
- **Concept**: When starting a focus session (such as "Online Learning" or "Coding"), FlowGuard can automatically launch or bring to focus the designated apps (e.g. launch VS Code and open your course browser tab).
- **Novice Benefit**: Removes the initial activation energy friction of hunting down and opening multiple work windows.

---

## 2. Session Focus Intention & Micro-Tasks Checklist
- **Concept**: When initiating a focus block, prompt the user with a single, optional question: *"What is the one primary outcome you want to complete in this session?"*
- **Execution**:
  - Displays the goal prominently in the active session header.
  - Allows breaking it down into 2-3 checkable subtasks.
  - Archives completed goals alongside session history to review tangible productivity gains over time.

---

## 3. High-Velocity Distraction Loop Alarm
- **Concept**: If FlowGuard detects more than 3 distinct hops to short-form media (YouTube Shorts, Instagram Reels, TikTok) within a 15-minute window, it triggers an escalated mindful intervention.
- **Novice Benefit**: Short-form algorithmic media is designed to exploit subconscious compulsion. A gentle, empathetic full-screen "Breathing Pause" screen allows users to break unconscious doomscrolling loops.

---

## 4. Lightweight Browser Extension / Companion Hook
- **Concept**: A minimal browser extension for Chrome, Edge, Brave, and Firefox that communicates via local loopback port (`127.0.0.1:3124`).
- **Capabilities**:
  - Distinguishes educational long-form videos (`youtube.com/watch?v=...`) from infinite scroll shorts (`youtube.com/shorts/...`), redirecting shorts back to the course tab.
  - Read active tab URLs directly without relying on OS window title scraping.

---

## 5. Windows System Tray Minimization & Ambient Status Icon
- **Concept**: Provide an option to minimize FlowGuard cleanly to the Windows notification tray near the clock.
- **Capabilities**:
  - A subtle tray icon changing color (Green = In Flow, Amber = In Grace Period, Blue = Idle).
  - Native Windows desktop toast notifications when target session durations are achieved.

---

## 6. Pomodoro Rhythm & Smart Ergonomic Break Coach
- **Concept**: Built-in 25/5 or 50/10 Pomodoro rhythm toggles with guided micro-breaks.
- **Ergonomics**: Includes 20-20-20 eye strain relief reminders, posture prompts, and hydration alerts during scheduled break intervals.

---

## 7. Weekly Focus Health & Distraction Heatmap (Exportable PDF/JSON)
- **Concept**: An automated weekly review dashboard visualizing focus peaks, most common drift triggers, and day-by-day pull-back resilience trends.
- **Privacy Assurance**: 100% rendered locally with zero external network requests.
