# ActivityWatch Integration Guide

This guide details the integration architecture between FlowGuard and [ActivityWatch](https://activitywatch.net/).

---

## 1. Official Documentation References

- [ActivityWatch Architecture](https://docs.activitywatch.net/en/latest/architecture.html)
- [ActivityWatch REST API](https://docs.activitywatch.net/en/latest/api/rest.html)
- [ActivityWatch Watchers](https://docs.activitywatch.net/en/latest/watchers.html)
- [Buckets and Events](https://docs.activitywatch.net/en/latest/buckets-and-events.html)
- [Configuration Guide](https://docs.activitywatch.net/en/latest/configuration.html)

---

## 2. ActivityWatch Components: Watchers vs. Server

ActivityWatch operates as a multi-process architecture:
1. **`aw-server`**: A local web server and datastore (storing events in SQLite). Listens by default on `127.0.0.1:5600`.
2. **`aw-qt`**: A tray management utility that starts and stops `aw-server` and default watchers.
3. **`aw-watcher-window`**: Desktop process that polls active window titles and application names every few seconds and sends heartbeats to `aw-server`.
4. **`aw-watcher-afk`**: Observes keyboard and mouse input to record whether the user is active or AFK.
5. **`aw-watcher-web`**: Optional browser extension (Chrome, Firefox) observing active tab URLs and titles.

**Critical Architectural Distinction**:
`aw-server` may be running and responding to HTTP requests even if watchers (`aw-watcher-window`) have crashed or stopped. FlowGuard specifically validates both server reachability AND event freshness to avoid using outdated telemetry.

---

## 3. Endpoints Used by FlowGuard

FlowGuard interacts only with documented, non-destructive read endpoints:

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/0/info` | `GET` | Handshake: validates service identity, version, hostname, and dev/testing mode. |
| `/api/0/buckets/` | `GET` | Bucket discovery: returns metadata for all available watchers. |
| `/api/0/buckets/{id}/events?limit=1` | `GET` | Event retrieval: fetches ONLY the single latest event for each watcher. |

FlowGuard **never** creates buckets, posts events, deletes buckets, or modifies SQLite datastores.

---

## 4. Bucket Discovery Algorithm

ActivityWatch generates bucket IDs containing machine-specific hostnames (e.g., `aw-watcher-window_DESKTOP-12345`). FlowGuard resolves buckets dynamically using metadata inspection:

```text
GET /api/0/buckets/
   |
   +--> For each bucket:
   |      - Matches type: "currentwindow", "afkstatus", or "web.tab.current"
   |      - Filters by server hostname returned during handshake
   |      - Verifies against explicit overrides in flowguard.toml (if provided)
   |
   +--> If multiple matching buckets exist on the same host:
          - Records ambiguity warning diagnostic
          - Selects primary candidate without crashing
```

---

## 5. Port Configuration and Port Conflicts

ActivityWatch uses port 5600 by default.

### Case A: Custom Port
If ActivityWatch is configured on a custom port (e.g. 5601):
1. In `config/flowguard.toml` (or `%APPDATA%\flowguard\config.toml`):
   ```toml
   [activity_source.activitywatch]
   base_url = "http://127.0.0.1:5601"
   ```
2. FlowGuard connects directly to `http://127.0.0.1:5601`.

### Case B: Port Collision / Unrelated Service
If another local service (e.g., a dev web server or proxy) occupies port 5600:
- FlowGuard queries `/api/0/info`.
- When the response fails to return valid ActivityWatch version and hostname fields, FlowGuard reports `SourceHealth::WrongService` and outputs clear troubleshooting instructions.

---

## 6. How to Inspect Raw ActivityWatch Events

To inspect what ActivityWatch is recording on your system:

### 1. View ActivityWatch Web Dashboard
Open `http://127.0.0.1:5600` in any web browser.

### 2. Inspect Raw Server Info via cURL / PowerShell
```powershell
curl.exe http://127.0.0.1:5600/api/0/info
```

### 3. List All Buckets
```powershell
curl.exe http://127.0.0.1:5600/api/0/buckets/
```

### 4. Fetch the Latest Window Event
```powershell
curl.exe "http://127.0.0.1:5600/api/0/buckets/aw-watcher-window_YOURHOSTNAME/events?limit=1"
```

---

## 7. Diagnosing Connection Failures

Run the FlowGuard doctor:
```powershell
flowguard doctor
```

Common scenarios and diagnostics:
- **`Disconnected (Service unreachable)`**: ActivityWatch is not running. Launch `aw-qt.exe` or start ActivityWatch from the Windows Start menu.
- **`Wrong Service`**: Another application is using port 5600. Update `base_url` in FlowGuard config to match ActivityWatch's port.
- **`Degraded (STALE)`**: ActivityWatch server is active, but the window watcher has paused or terminated. Check Task Manager to confirm `aw-watcher-window.exe` is running.
- **`Auth Required`**: Authentication is enabled on ActivityWatch. Set the `FLOWGUARD_ACTIVITYWATCH_API_KEY` environment variable.
