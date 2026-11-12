# FlowGuard Testing Guide

This document describes the automated test architecture and verification procedures for FlowGuard.

---

## 1. Test Architecture

The test suite is structured into six independent layers:

```text
tests/
├── diagnostics_tests.rs          # Provider-neutral diagnostics & architecture boundary tests (5 tests)
├── discovery_tests.rs            # Ambiguous bucket selection & override precedence tests (15 tests)
├── domain_mapper_tests.rs        # Event normalization, AFK semantics, capability separation & timeouts (30 tests)
├── mock_scenarios.rs             # WireMock integration tests covering all 24 network/service scenarios (24 tests)
├── unicode_truncation_tests.rs   # Regression tests for Unicode-safe truncation & display width (15 tests)
└── real_integration_test.rs      # Read-only validation against a live local ActivityWatch instance (1 test)
```

**Total automated tests**: **90 passed, 0 failed, 0 ignored**.

---

## 2. Provider-Neutral Diagnostics Tests (`tests/diagnostics_tests.rs`)

A 5-test suite verifying the provider-neutral diagnostic model:
1. `test_01_valid_activitywatch_diagnostic_report`: validates that `ActivityWatchAdapter::diagnose()` produces a clean, provider-neutral `DiagnosticReport` without leaking internal types.
2. `test_02_unavailable_server_produces_useful_diagnostics`: verifies that connection failures produce a complete report with `is_reachable: false` and actionable troubleshooting advice rather than panicking.
3. `test_03_unrelated_service_produces_wrong_service_diagnostic`: validates signature verification against non-ActivityWatch services.
4. `test_04_missing_essential_window_watcher_reported`: verifies that missing window watchers report `SubsystemStatus::Unavailable` with actionable details.
5. `test_05_mocked_non_activitywatch_source_runs_through_activity_service`: architectural boundary proof demonstrating that `ActivityService::run_doctor()` works transparently with a mocked `NativeWindowsAdapter`.

---

## 3. Bucket Discovery & Ambiguity Tests (`tests/discovery_tests.rs`)

A 15-test suite verifying the strict 5-step bucket selection policy:
1. `test_01_single_candidate_selection`: exactly one candidate on host is selected cleanly.
2. `test_02_explicit_valid_override_takes_precedence`: valid configured override supersedes auto-discovery.
3. `test_03_override_referencing_missing_bucket_fails_cleanly`: override referencing non-existent bucket returns `None` and records diagnostic.
4. `test_04_override_referencing_wrong_category_fails_cleanly`: override referencing mismatched event category returns `None` and records diagnostic.
5. `test_05_multiple_candidates_with_one_canonical_bucket_selects_canonical`: multiple candidates on host with exactly one canonical bucket (`aw-watcher-window_{host}`) selects canonical bucket.
6. `test_06_multiple_canonical_candidates_fails_cleanly`: multiple canonical candidates returns `None` and records diagnostic (never guesses).
7. `test_07_multiple_non_canonical_candidates_fails_cleanly`: multiple non-canonical candidates returns `None` and records diagnostic (never guesses).
8. `test_08_deterministic_behavior_across_repeated_runs`: running discovery repeatedly with identical HashMap returns identical results every time.
9. `test_09_no_matching_candidate`: handles zero matching buckets cleanly.
10. `test_10_multiple_hosts_filtered_by_server_hostname`: ignores candidates from other hostnames.
11. `test_11_essential_window_ambiguity`: ambiguous window candidates return `None` and flag degraded source health.
12. `test_12_essential_afk_ambiguity`: ambiguous AFK candidates return `None` and disable idle tracking.
13. `test_13_optional_browser_ambiguity`: ambiguous browser candidates return `None` without degrading desktop monitoring.
14. `test_14_official_aw_watcher_web_supports_incognito`: official client verifies incognito capability.
15. `test_15_custom_web_client_unknown_incognito_support`: custom browser client safely defaults incognito detection to `false`.

---

## 4. Domain, AFK & Capabilities Test Suite (`tests/domain_mapper_tests.rs`)

A 30-test suite verifying event normalization, AFK duration arithmetic, capability/freshness decoupling, and transport failure differentiation:
- **AFK Semantics (Tests 01-09)**: Fresh active/afk states, ongoing AFK heartbeats, reading pauses, stale AFK detection, and AFK health invariance.
- **Browser Isolation (Test 10)**: Stale or missing browser data never degrades healthy desktop monitoring.
- **Capability Separation**: Verifies that quiescent or stale browser events retain `capabilities.browser_url_tracking == true` while `browser_freshness == FreshnessState::Stale`.
- **Transport Failure Distinction**:
  * Window watcher timeout: marked as `FreshnessState::Stale`, health `Degraded`, with timeout duration diagnostic.
  * Browser watcher timeout: marked as `FreshnessState::Stale`, while desktop health remains `Connected`.
  * AFK watcher timeout: marked as `IdleState::Unknown`, health `Degraded`.
  * HTTP 404 missing bucket: distinguished from empty bucket `[]`.

---

## 5. 24 Mock Integration Scenarios (`tests/mock_scenarios.rs`)

All 24 required scenarios verified using an isolated local HTTP mock server (WireMock):
- Handshake & Server Validation (`test_01`, `test_02`).
- Network & Transport Failure Modes (`test_03_connection_refused`, `test_04_request_timeout`, `test_16_authorization_error`, `test_17_custom_configured_port`, `test_18_reconnection`, `test_19_http_500`).
- Bucket Discovery Scenarios (`test_05_missing_window_bucket`, `test_06_missing_afk_bucket`, `test_07_missing_browser_bucket`, `test_08_multiple_matching_buckets_ambiguity`, `test_20_http_404_missing_bucket_triggers_rediscovery`).
- Partial Failures & Isolation (`test_21_partial_failure_window_fails_afk_succeeds`, `test_22_partial_failure_browser_fails_desktop_healthy`).
- Event Normalization Scenarios (`test_09_malformed_event_json`, `test_10_missing_event_fields`, `test_11_stale_window_event`, `test_12_stale_browser_event`, `test_13_valid_application_event`, `test_14_valid_afk_event`, `test_15_valid_browser_event`, `test_23_empty_event_response_distinguished_from_timeout`, `test_24_no_stale_window_data_presented_as_current`).

---

## 6. Unicode Truncation Regression Suite (`tests/unicode_truncation_tests.rs`)

A 15-test suite verifying panic-free string truncation across varied international and special Unicode text:
- Mathematical characters (`'𝒍'`, `𝒳`), emoji (single, ZWJ sequences, flags), accented characters (`é`, `ü`), CJK ideographs, Hindi Devanagari, Arabic RTL, and Cyrillic.
- Exact reproduction of the byte index 37 crash (`test_13_exact_regression_reproduction`).

---

## 7. Running the Test Suite

```powershell
# Run all automated tests
cargo test

# Run specific test suites
cargo test --test diagnostics_tests
cargo test --test discovery_tests
cargo test --test domain_mapper_tests
cargo test --test mock_scenarios
cargo test --test unicode_truncation_tests

# Run clippy check with strict zero warnings
cargo clippy --all-targets -- -D warnings
```
