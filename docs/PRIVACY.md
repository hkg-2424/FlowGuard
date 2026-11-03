# FlowGuard Privacy Architecture

## 1. Local-First Guarantee

FlowGuard is built from the ground up as a **100% local-first application**. It runs completely on your personal computer.

There is:
- **No telemetry** sent to any external server.
- **No cloud synchronization** of activity or analytics.
- **No remote error reporting** service.
- **No remote AI or cloud LLM calls** in M1.

---

## 2. What FlowGuard Collects (and Reads)

During Milestone 1, FlowGuard reads strictly what is necessary to understand the user's current context:

1. **Active Application Executable**: (e.g. `chrome.exe`, `code.exe`).
2. **Active Window Title**: (e.g. `src/main.rs - FlowGuard - Visual Studio Code`).
3. **User Idle State**: Binary active or away-from-keyboard (AFK) status from ActivityWatch's idle monitor.
4. **Browser Tab Information (Optional)**: Tab URL and web page title **only** when an optional ActivityWatch web extension is active and installed by the user.

---

## 3. What FlowGuard NEVER Collects

FlowGuard does **NOT**:
- Take screenshots or capture video of the desktop.
- Record keystrokes or keyboard input (no keylogging).
- Record mouse coordinates or movement patterns.
- Read or inspect clipboard contents.
- Inspect form fields or passwords.
- Read file contents from your workspace or drive.
- Independently crawl browser history or personal bookmarks.
- Duplicate or persist raw ActivityWatch events in a second background database.

---

## 4. Privacy Protections in Practice

- **Query Redaction**: In streaming console mode, sensitive URL query parameters (e.g., authentication tokens or search parameters) are automatically sanitized unless `--verbose` is explicitly specified.
- **Safe Truncation**: Overly long window titles that might inadvertently contain sensitive file paths or documents are truncated in standard views.
- **Secrets Protection**: Optional API keys for ActivityWatch are read from the environment and are never logged or stored in plaintext.

