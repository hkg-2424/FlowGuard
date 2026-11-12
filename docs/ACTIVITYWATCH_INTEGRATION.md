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

## 4. Bucket Discovery & Canonical Selection Algorithm (M1.1)

ActivityWatch generates bucket IDs containing machine-specific hostnames (e.g., `aw-watcher-window_DESKTOP-12345`). FlowGuard resolves buckets dynamically using a strict 5-step policy:

```text
GET /api/0/buckets/
   |
   +--> Step 1: Valid Explicit Override
   |      - If configured in flowguard.toml under [activity_source.activitywatch.bucket_overrides]
   |      - Validates that the bucket exists in ActivityWatch and matches expected category.
   |      - If missing or wrong category: returns None + explicit diagnostic (never guesses).
   |
   +--> Step 2: Single Candidate on Host
   |      - Filters buckets matching predicate and matching server_hostname.
   |      - If exactly 1 candidate matches: selects it immediately.
   |
   +--> Step 3: Unique Canonical Match
   |      - If multiple candidates match on host: checks for unique canonical pattern:
   |        * Window: "aw-watcher-window_{host}"
   |        * AFK: "aw-watcher-afk_{host}"
   |        * Web: "aw-watcher-web-*_{host}"
   |      - If exactly 1 matches canonical pattern: selects it and records informative diagnostic.
   |
   +--> Step 4 & 5: Unresolvable Ambiguity Protection
          - If multiple canonical candidates exist or no canonical candidate exists:
          - Returns None (NEVER guesses or arbitrarily picks candidates[0]).
          - Records explicit diagnostic listing competing buckets and instructs user to configure override.
```

---

## 5. Transport Error Differentiation & WatcherResult (M1.1)

In early versions, HTTP transport errors, timeouts, and missing buckets were masked as empty buckets (`Ok(None)`), producing misleading diagnostics like "Bucket has no events recorded yet".

In M1.1, FlowGuard translates query results into a strongly typed `WatcherResult`:

```rust
pub enum WatcherResult<'a> {
    Present(&'a AwRawEvent),
    Empty,
    NotConfigured,
    Timeout(u64),
    MissingBucket(String),
    FetchError(String),
}
```

### Error Classification Rules:
1. **Empty Bucket (`[]`)**: Returns `WatcherResult::Empty`. Freshness is `Missing`. Diagnostic: `"Bucket has no recorded events yet"`.
2. **Connection Timeout (`SourceError::Timeout`)**: Returns `WatcherResult::Timeout(ms)`. Freshness is `Stale` (unreliable observation). Diagnostic: `"Request timed out after ...ms"`. Source health is marked `Degraded`.
3. **HTTP 404 (`SourceError::MissingRequiredBucket`)**: Returns `WatcherResult::MissingBucket(id)`. Freshness is `Missing`. Diagnostic: `"Bucket ... was not found in ActivityWatch"`.
4. **Connection Refused (`SourceError::ConnectionRefused`)**: Source health immediately marked `Disconnected`.
5. **Browser Extension Timeout**: Flagged as unavailable with an explicit diagnostic; desktop window and AFK tracking remain operational (`Connected`).

---

## 6. Port Configuration and Port Conflicts

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

## 7. How to Inspect Raw ActivityWatch Events

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
