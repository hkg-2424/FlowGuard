# FlowGuard Testing Guide

This document describes the automated test architecture and verification procedures for FlowGuard Milestone 1.

---

## 1. Test Architecture

The test suite is structured into three layers to guarantee independence from live environments:

```text
tests/
├── domain_mapper_tests.rs   # Pure unit tests for event normalization & freshness logic
├── mock_scenarios.rs        # WireMock integration tests covering all 18 network/service scenarios
└── real_integration_test.rs # Optional read-only validation against a live ActivityWatch instance
```

---

## 2. 18 Mock Integration Scenarios (`tests/mock_scenarios.rs`)

All 18 required scenarios are verified using an isolated local HTTP mock server without depending on an external ActivityWatch installation:

| # | Scenario | Verification Method |
|---|---|---|
| **01** | Valid ActivityWatch Server Info | Mocks `/api/0/info` with version `v0.14.1` and hostname; verifies successful handshake and capabilities parsing. |
| **02** | Unrelated HTTP Service (Wrong Service) | Mocks an unrelated API returning 200 with generic JSON; verifies `SourceError::WrongService` rejection and health state. |
| **03** | Connection Refused | Connects to an unopened/refused port; verifies `SourceError::ConnectionRefused` and `SourceHealth::Disconnected`. |
| **04** | Request Timeout | Simulates delayed responses exceeding `request_timeout_ms`; verifies `SourceError::Timeout`. |
| **05** | Missing Window Bucket | Mocks bucket list omitting `currentwindow`; verifies graceful degradation and `FreshnessState::Missing`. |
| **06** | Missing AFK Bucket | Mocks bucket list with window only; verifies `IdleState::Unknown` and `capabilities.idle_tracking == false`. |
| **07** | Missing Browser Bucket | Mocks setup without web extension; verifies `obs.browser == None` without crashing. |
| **08** | Multiple Matching Buckets | Mocks two window buckets on the same host; verifies diagnostic alert and primary bucket selection. |
| **09** | Malformed Event JSON | Returns corrupt non-JSON strings; verifies `SerializationError` recovery and missing event fallback. |
| **10** | Missing Event Fields | Returns event data with empty JSON objects `{}`; verifies safe `None` parsing without panic. |
| **11** | Stale Window Event | Returns window events with timestamps older than threshold; verifies `FreshnessState::Stale` and suppressed app name. |
| **12** | Stale Browser Event | Returns browser events with timestamps 1 hour old; verifies `browser == None` and `capabilities.browser_url_tracking == false`. |
| **13** | Valid Application Event | Returns fresh `code.exe` event with title; verifies normalized fields. |
| **14** | Valid AFK Event | Returns `status: "afk"`; verifies `IdleState::Afk`. |
| **15** | Valid Browser Event | Returns YouTube tutorial tab with `audible: true`; verifies `BrowserContext` structure. |
| **16** | Authorization Error | Returns HTTP 401 Unauthorized; verifies `SourceError::AuthRequired`. |
| **17** | Custom Configured Port | Verifies connection to custom dynamic ports via configured `base_url`. |
| **18** | Reconnection After Failure | Simulates server outage followed by recovery; verifies automatic reconnection during polling. |

---

## 3. Running the Test Suite

```powershell
# Run all automated tests
cargo test

# Run only the domain mapper unit tests
cargo test --test domain_mapper_tests

# Run only the mock scenario integration tests
cargo test --test mock_scenarios

# Run tests with output printed to console
cargo test -- --nocapture
```

---

## 4. Live Real-World Integration Test

The test in `tests/real_integration_test.rs` automatically detects if a local ActivityWatch server is active at `http://127.0.0.1:5600`.
- If running: connects, discovers buckets, queries latest activity, and verifies capabilities without writing any data.
- If not running: cleanly skips without failing the build.
