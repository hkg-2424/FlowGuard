# FlowGuard Roadmap

This roadmap defines the iterative development stages of FlowGuard. Milestones must be executed sequentially to ensure each layer is thoroughly tested, validated, and documented before building upon it.

---

### M1 — ActivityWatch Integration Foundation (Current Milestone)
- External ActivityWatch integration via local REST API.
- Decoupled `ActivitySource` trait abstraction.
- Normalized domain observation models (`ActivityObservation`, `BrowserContext`, `IdleState`, `ActivityCapabilities`).
- Handshake identity verification & port collision protection.
- Dynamic bucket discovery & independent staleness tracking.
- CLI diagnostics and change stream monitor (`flowguard doctor`, `flowguard status`, `flowguard observe`).

---

### M2 — Workflow Engine
- Formal workflow definitions (e.g. "Competitive Programming", "React Study", "Deep Coding").
- Rules for allowed applications, allowed website domains, and specific resource paths.
- Intent declaration by the user (target session duration, active workflow).
- Workflow matching engine evaluating normalized `ActivityObservation` against active workflow constraints.

---

### M3 — Desktop Application
- Modern desktop user interface using Tauri 2, React, TypeScript, Tailwind CSS, and shadcn/ui.
- System tray integration with quick workflow toggles.
- Real-time status display showing active workflow, observed application, and current focus health.
- Local SQLite database for persistent user preferences and defined workflows.

---

### M4 — Intervention Engine
- Non-intrusive focus protection mechanisms.
- Grace periods before alerting on context departures.
- Visual warnings and gentle nudges back to the active workflow.
- Commitment mode with optional strict blocking.
- Emergency override mechanisms to prevent user lock-out.

---

### M5 — Browser Intelligence
- Resource-level tracking (distinguishing specific YouTube tutorials from unrelated videos).
- Dedicated optional FlowGuard browser extension (Chrome, Edge, Firefox).
- Fine-grained URL and active tab inspection.
- Explicit, user-permissioned Incognito mode handling.

---

### M6 — Behavioural Productivity Features
- Distraction Journal: quick prompt asking why the user switched context.
- Research Parking Lot: quick capture of unrelated ideas without breaking flow.
- Learning Checkpoints: periodic knowledge retention prompts.
- Return-to-Work Assistant: re-orienting the user after returning from an AFK break.
- Local focus analytics and productivity dashboards.

---

### M7 — Native Activity Monitoring
- Independent native Rust observation engine leveraging Windows native APIs:
  - Win32 `GetForegroundWindow`, `GetWindowTextW`, `QueryFullProcessImageNameW`.
  - Windows UI Automation / Accessibility for browser title tracking.
  - Windows `GetLastInputInfo` for high-precision idle detection.
- Seamless drop-in replacement: `NativeWindowsAdapter` implements the identical `ActivitySource` trait created in M1, allowing FlowGuard to operate with zero external dependencies.
