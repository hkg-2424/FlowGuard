# FlowGuard — Project Status (Milestone 1.1)

**Current Milestone**: M1.1 — Architectural and Data-Correctness Corrections (Batch 1)
**Status**: **COMPLETE**  
**Date**: November 4, 2026

---

## 1. What Has Been Implemented

1. **Provider-Neutral Diagnostic Abstraction (`ports.rs`, `diagnostics.rs`)**:
   - Added `fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError>` to `ActivitySource` trait.
   - Introduced domain models `DiagnosticReport`, `SubsystemDiagnostic`, and `SubsystemStatus` in [`src/domain/diagnostics.rs`](file:///d:/FlowGuardAG/src/domain/diagnostics.rs).
   - Completely decoupled `ActivityService::run_doctor(&mut self)` from ActivityWatch internals. `run_doctor()` now delegates cleanly to `self.source.diagnose()`.
   - Updated `print_doctor_report()` in [`src/presentation/console.rs`](file:///d:/FlowGuardAG/src/presentation/console.rs) to display provider-neutral diagnostic reports without leaking ActivityWatch bucket IDs or HTTP concepts into application logic.

2. **Decoupled Capabilities from Freshness (`mapper.rs`, `discovery.rs`)**:
   - Fixed architectural defect where transiently stale browser events mutated `capabilities.browser_url_tracking` to `false`.
   - Established invariant: `ActivityCapabilities` reflects structural availability determined during bucket discovery or hardware hook initialization.
   - Freshness remains tracked independently per channel (`window_freshness`, `idle_freshness`, `browser_freshness`).
   - If a user switches to a terminal and browser events become stale, `browser_freshness` is `Stale`, but `capabilities.browser_url_tracking` remains `true` (Available).

3. **Safe Bucket Selection Algorithm (`discovery.rs`)**:
   - Replaced arbitrary `candidates[0]` selection with a strict 5-step policy:
     1. Valid explicit override (`bucket_overrides` in `flowguard.toml`), validated for existence and category.
     2. Exactly one candidate matching predicate and host.
     3. Unique canonical match (`aw-watcher-window_{host}`, `aw-watcher-afk_{host}`, `aw-watcher-web-*_{host}`).
     4. If ambiguity cannot be resolved, returns `None` (never guesses).
     5. Emits an explicit diagnostic listing all competing candidates and instructing the user to configure `bucket_overrides`.

4. **Transport Error Differentiation & `WatcherResult` (`mapper.rs`, `mod.rs`)**:
   - Eliminated masking of HTTP failures as missing events.
   - Introduced `WatcherResult` enum: `Present(&AwRawEvent)`, `Empty`, `NotConfigured`, `Timeout(u64)`, `MissingBucket(String)`, `FetchError(String)`.
   - Distinguishes empty buckets (`[]`) from timeouts, 404s, connection failures, 5xx, and serialization errors.
   - Window watcher timeout marks observation freshness as `Stale` (unreliable observation) and source health as `Degraded` with an explicit latency diagnostic.
   - Browser watcher timeout marks browser context as unavailable while desktop monitoring remains `Connected`.

5. **Existing M1 Core & Unicode Safety**:
   - Panic-free Unicode text truncation utility in [`src/presentation/truncate.rs`](file:///d:/FlowGuardAG/src/presentation/truncate.rs) using `unicode-segmentation` and `unicode-width`.
   - Replaceable `ActivitySource` port and ActivityWatch adapter.

---

## 2. Test Suite Status

The automated test suite has expanded from 57 to **90 tests**, all passing:

1. **Provider-Neutral Diagnostics Tests (`tests/diagnostics_tests.rs`)**: **5 tests**
   - Valid generic diagnostic report produced by ActivityWatchAdapter.
   - Unavailable server produces useful diagnostics without panicking.
   - Unrelated HTTP service produces wrong-service diagnostic.
   - Missing essential window watcher reported in subsystems.
   - Mocked non-ActivityWatch `ActivitySource` runs through `ActivityService::run_doctor()`.

2. **Ambiguous Bucket Selection Tests (`tests/discovery_tests.rs`)**: **15 tests**
   - Single candidate selection.
   - Explicit valid override precedence.
   - Explicit override referencing missing bucket.
   - Explicit override referencing wrong category.
   - Multiple candidates with one canonical bucket.
   - Multiple canonical candidates fail cleanly (`None` + diagnostic).
   - Multiple non-canonical candidates fail cleanly (`None` + diagnostic).
   - Deterministic discovery behavior across repeated runs.
   - No matching candidate.
   - Multiple hosts filtered by server hostname.
   - Essential window bucket ambiguity.
   - Essential AFK bucket ambiguity.
   - Optional browser bucket ambiguity.
   - Official `aw-watcher-web` incognito detection verification.
   - Custom web client unknown incognito support verification.

3. **Domain, AFK & Capabilities Tests (`tests/domain_mapper_tests.rs`)**: **30 tests**
   - AFK state & source health decoupling (all 10 specification scenarios).
   - Capability independence from freshness (stale browser preserves `capabilities.browser_url_tracking`).
   - Window watcher timeout marks observation `Stale` and source `Degraded`.
   - Browser timeout retains desktop health `Connected`.
   - AFK timeout marks `IdleState::Unknown` and source `Degraded`.
   - HTTP 404 missing bucket distinguished from empty bucket `[]`.

4. **Mock Integration Scenarios (`tests/mock_scenarios.rs`)**: **24 tests**
   - Full WireMock coverage of handshake, port conflicts, timeouts, missing buckets, corrupt payloads, reconnection, HTTP 500 API errors, 404 rediscovery, and partial failure isolation.

5. **Unicode Truncation Regression Suite (`tests/unicode_truncation_tests.rs`)**: **15 tests**
   - Mathematical scripts (`'𝒍'`), emoji sequences, combining accents, CJK, and exact reproduction of the byte-slicing crash.

6. **Live Integration Test (`tests/real_integration_test.rs`)**: **1 test**
   - Read-only verification against the live local ActivityWatch server.

**Total Test Count**: **90 passed, 0 failed, 0 ignored**.

---

## 3. Real-World Live Verification on Windows 11

- `flowguard.exe doctor`: Handshake succeeded against ActivityWatch v0.14.1 on `GRAVITY`, discovered desktop window and AFK buckets as `Healthy`, web bucket reported as `Unavailable` (not installed), overall health `Connected (Healthy)`.
- `flowguard.exe status`: Endpoint, Health `Connected`, capabilities correctly summarized.
- `flowguard.exe observe --once`: Captured live active window (`brave.exe - Optimize AI Plans`), `IdleState::Active`, freshness `Fresh`.

---

## 4. Next Steps

- **Batch 2 Scope** (to be executed upon user review):
  - URL parser sanitization.
  - Stepped sleep for responsive Ctrl+C shutdown.
  - Event duration numerical bounds and clamping.
- **Milestone 2**:
  - Workflow engine and focus tracking.
