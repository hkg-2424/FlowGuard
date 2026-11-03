# FlowGuard — Project Status (Milestone 1)

**Current Milestone**: M1 — ActivityWatch Integration Foundation  
**Status**: **COMPLETE**  
**Date**: November 3, 2026

---

## 1. What Has Been Implemented

1. **Replaceable Activity Source Architecture**:
   - `ActivitySource` trait in [`src/application/ports.rs`](file:///d:/FlowGuardAG/src/application/ports.rs).
   - Domain models in [`src/domain/`](file:///d:/FlowGuardAG/src/domain/): `ActivityObservation`, `BrowserContext`, `IdleState`, `FreshnessState`, `ActivityCapabilities`, and `SourceHealth`.
2. **ActivityWatch Adapter**:
   - Reusable HTTP client with timeout, port customization, and optional token auth in [`src/infrastructure/activitywatch/client.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/client.rs).
   - Server verification via `GET /api/0/info` detecting valid ActivityWatch signatures vs. unrelated services (`WrongService`).
   - Dynamic bucket discovery in [`src/infrastructure/activitywatch/discovery.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/discovery.rs) with hostname matching and ambiguity alerts.
   - Independent watcher freshness engine in [`src/infrastructure/activitywatch/mapper.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/mapper.rs) calculating event age with duration arithmetic.
3. **Unicode-Safe Console Presentation & Reliability**:
   - Panic-free Unicode text truncation utility in [`src/presentation/truncate.rs`](file:///d:/FlowGuardAG/src/presentation/truncate.rs) using `unicode-segmentation` and `unicode-width`.
   - Protects console output from byte-slicing panics caused by 4-byte mathematical scripts (e.g. `'𝒍'`), emojis, and double-width CJK characters.
   - Replaced all byte slicing across the presentation layer.
4. **Application & CLI**:
   - `ActivityService` handling polling loops, bounded reconnection backoff, and state change detection.
   - CLI subcommands:
     - `flowguard doctor`: Deep diagnostic inspection of endpoint, handshake, buckets, and watcher freshness.
     - `flowguard status`: Operational health and capabilities summary.
     - `flowguard observe --once`: Single snapshot output.
     - `flowguard observe`: Real-time change stream with graceful Ctrl+C shutdown.
4. **Configuration**:
   - Hierarchical configuration loader (`--config`, local `flowguard.toml`, `%APPDATA%\flowguard\config.toml`, fallback defaults).
   - Strict validation preventing invalid URLs or aggressive sub-100ms polling intervals.

---

## 2. What Has Been Tested

1. **Automated Unit, Mock & Regression Tests**:
   - **9 Domain Mapper Unit Tests**: Verified timestamp duration arithmetic, threshold transitions, missing fields, and change detection.
   - **18 Mock HTTP Integration Tests**: Verified all network failure modes, port conflicts, timeouts, missing buckets, corrupt payloads, and reconnection.
   - **15 Unicode Truncation Regression Tests**: Verified ASCII, mathematical scripts (`𝒍`), emoji, accented letters, non-English scripts (Japanese, Hindi, Arabic, Cyrillic), combining accents, boundary characters, and exact reproduction of the byte-slicing crash.
   - **1 Live Integration Test**: Verified read-only integration against running ActivityWatch.
   - Total automated test suite: **43 passed, 0 failed, 0 ignored**.
2. **Real-World Live Testing on Windows 11**:
   - Verified live handshake against local ActivityWatch instance at `http://127.0.0.1:5600`.
   - Verified observation streaming with applications (`brave.exe`), switching across browser tabs containing mathematical and international Unicode titles without crashing.
   - **Idle Transition Investigation**: Confirmed that `[?IDLE ] ... (Degraded)` was caused by an initial `Stale` evaluation on startup when the AFK watcher had not yet emitted a heartbeat within the 15s window, which transitioned to `[ACTIVE]` as soon as the fresh heartbeat arrived. Validated that idle status (`Active`/`Afk`/`Unknown`) and source health (`Connected`/`Degraded`) remain strictly separated in domain logic. An AFK user with an active watcher reports `Connected`, not `Degraded`.
3. **Resource Benchmarks**:
   - Process Working Set: **14.64 MB** average.
   - CPU utilization during 1000ms polling: **< 0.05% CPU**.

---

## 3. Known Limitations (By Design in M1)

- **External Dependency**: ActivityWatch must be manually started by the user. FlowGuard does not manage the lifecycle of `aw-qt` or `aw-server`.
- **Browser URL Tracking Requires Extension**: If the user does not have `aw-watcher-web` installed in their browser, browser tab URLs are marked as unavailable (`None`), though active application (`chrome.exe`) and window titles are still captured.
- **No GUI in M1**: All interactions are via the command-line interface.

---

## 4. Current ActivityWatch Dependency Summary

- FlowGuard connects to ActivityWatch exclusively via its documented local REST API (`/api/0/info`, `/api/0/buckets/`, `/api/0/buckets/{id}/events`).
- FlowGuard does **not** read ActivityWatch's SQLite database directly, copy source code, or write events into ActivityWatch.
- The `ActivitySource` trait completely isolates the domain layer from ActivityWatch schemas.

---

## 5. Next Milestone: M2 — Workflow Engine

Milestone 2 will introduce:
- Workflow definitions: allowed applications, allowed website domains, and specific tutorial/resource URLs.
- Intent declaration by the user (target duration, active workflow).
- Workflow matching engine evaluating incoming `ActivityObservation` snapshots against active workflow rules.
- Distinction between intentional context-switching (e.g. IDE to documentation) and true distractions.

