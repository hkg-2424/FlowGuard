# FlowGuard — Milestone 1 Acceptance Checklist

This document tracks the acceptance criteria defined in Section 23 ("Definition of Done") for Milestone 1.

---

## Acceptance Verification Matrix

| # | Acceptance Requirement | Status | Evidence / Verification Method |
|---|---|:---:|---|
| **1** | FlowGuard builds successfully | **PASS** | `cargo build` and `cargo build --release` compile with zero errors and zero warnings. |
| **2** | Connects to a real ActivityWatch instance | **PASS** | Successfully connected to local `http://127.0.0.1:5600` running ActivityWatch v0.14.1 on host `GRAVITY`. |
| **3** | Validates server using info endpoint | **PASS** | Handshake verifies `GET /api/0/info`, checks version, hostname, and profile; rejects non-ActivityWatch responses as `WrongService`. |
| **4** | Discovers buckets dynamically | **PASS** | Discovers `aw-watcher-window_GRAVITY` and `aw-watcher-afk_GRAVITY` dynamically by event type (`currentwindow`, `afkstatus`) and host matching without hardcoded IDs. |
| **5** | Reads latest window and available AFK/browser events | **PASS** | Uses `/api/0/buckets/{id}/events?limit=1` to retrieve the latest state for each watcher. |
| **6** | Maps events into FlowGuard domain models | **PASS** | Normalizes raw payloads into `ActivityObservation`, `BrowserContext`, `IdleState`, and `FreshnessState` via `mapper.rs`. |
| **7** | Exposes source capabilities and health | **PASS** | Dynamic capabilities (`application_tracking`, `window_title_tracking`, `idle_tracking`, `browser_url_tracking`, `incognito_detection`) and `SourceHealth` (`Connected`, `Degraded`, `Disconnected`, `WrongService`). |
| **8** | Handles missing services, missing watchers, wrong endpoints, and stale data | **PASS** | Tested in `tests/mock_scenarios.rs` (18 tests) and verified on live system (where stopped watchers were flagged as `Stale` after 15s). |
| **9** | Supports configurable server addresses | **PASS** | Configurable `base_url` in `config/flowguard.example.toml`, user `%APPDATA%`, or `--config` CLI argument. Verified via `test_17_custom_configured_port`. |
| **10** | Automated tests for major failure cases | **PASS** | 28 automated tests passing: 9 domain mapper unit tests, 18 mock integration tests, and 1 real integration test. |
| **11** | Resource consumption measured | **PASS** | Measured via [`scripts/benchmark.ps1`](file:///d:/FlowGuardAG/scripts/benchmark.ps1): 14.64 MB average working set memory, < 0.05% CPU during 1000ms polling. Recorded in [`docs/PERFORMANCE.md`](file:///d:/FlowGuardAG/docs/PERFORMANCE.md). |
| **12** | Required documentation exists and matches code | **PASS** | All documentation files (`ARCHITECTURE.md`, `CODE_MAP.md`, `ACTIVITYWATCH_INTEGRATION.md`, `PRIVACY.md`, `TESTING.md`, `PERFORMANCE.md`, `PROJECT_STATUS.md`, ADRs) generated and verified. |
| **13** | Explains how ActivityWatch will eventually be replaced | **PASS** | Documented in `ARCHITECTURE.md`, `ROADMAP.md`, and `ADR-002-replaceable-activity-source.md` with explicit mapping to future `NativeWindowsAdapter`. |

---

## Conclusion
All 13 acceptance requirements have been implemented, tested, measured, and verified. Milestone 1 is **COMPLETE**.
