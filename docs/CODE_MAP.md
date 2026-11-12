# FlowGuard — Code Map

This document serves as the primary developer reference for navigating and maintaining the FlowGuard codebase.

---

## 1. Quick Navigation Table

| If you want to modify... | Go to file | Primary Struct / Function |
|---|---|---|
| **ActivityWatch HTTP endpoint, timeouts, or auth** | [`src/infrastructure/activitywatch/client.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/client.rs) | `ActivityWatchClient` |
| **ActivityWatch handshake & server validation** | [`src/infrastructure/activitywatch/client.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/client.rs) | `ActivityWatchClient::handshake` |
| **Bucket discovery & selection rules** | [`src/infrastructure/activitywatch/discovery.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/discovery.rs) | `discover_buckets`, `select_bucket` |
| **Raw event parsing & JSON data structures** | [`src/infrastructure/activitywatch/models.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/models.rs) | `ServerInfo`, `BucketMetadata`, `AwRawEvent` |
| **Event mapping, WatcherResult & freshness** | [`src/infrastructure/activitywatch/mapper.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/mapper.rs) | `WatcherResult`, `check_freshness`, `map_events_to_observation` |
| **ActivityWatch adapter implementation** | [`src/infrastructure/activitywatch/mod.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/mod.rs) | `ActivityWatchAdapter`, `diagnose()` |
| **Source abstraction interface (Trait)** | [`src/application/ports.rs`](file:///d:/FlowGuardAG/src/application/ports.rs) | `pub trait ActivitySource`, `SourceError` |
| **Normalized activity domain model** | [`src/domain/activity.rs`](file:///d:/FlowGuardAG/src/domain/activity.rs) | `ActivityObservation`, `BrowserContext`, `IdleState`, `FreshnessState` |
| **Capability definitions** | [`src/domain/capabilities.rs`](file:///d:/FlowGuardAG/src/domain/capabilities.rs) | `ActivityCapabilities`, `SourceCapabilities` |
| **Provider-neutral diagnostics** | [`src/domain/diagnostics.rs`](file:///d:/FlowGuardAG/src/domain/diagnostics.rs) | `DiagnosticReport`, `SubsystemDiagnostic`, `SubsystemStatus` |
| **Source health definitions** | [`src/domain/source_health.rs`](file:///d:/FlowGuardAG/src/domain/source_health.rs) | `SourceHealth` |
| **Polling orchestration & change detection** | [`src/application/activity_service.rs`](file:///d:/FlowGuardAG/src/application/activity_service.rs) | `ActivityService::observe`, `run_doctor` |
| **CLI command implementations & output format** | [`src/presentation/console.rs`](file:///d:/FlowGuardAG/src/presentation/console.rs) | `print_doctor_report`, `print_observation_change` |
| **Unicode-safe text truncation** | [`src/presentation/truncate.rs`](file:///d:/FlowGuardAG/src/presentation/truncate.rs) | `truncate_str`, `truncate_to_width`, `truncate_to_graphemes` |
| **CLI arguments & argument parsing** | [`src/main.rs`](file:///d:/FlowGuardAG/src/main.rs) | `struct Cli`, `enum Commands` |
| **Configuration schema & validation** | [`src/config.rs`](file:///d:/FlowGuardAG/src/config.rs) | `FlowGuardConfig`, `ActivityWatchConfig` |
| **Example configuration file** | [`config/flowguard.example.toml`](file:///d:/FlowGuardAG/config/flowguard.example.toml) | TOML template |
| **Ambiguous bucket selection tests (15 tests)** | [`tests/discovery_tests.rs`](file:///d:/FlowGuardAG/tests/discovery_tests.rs) | `test_01_*` through `test_15_*` |
| **Provider-neutral diagnostics tests (5 tests)** | [`tests/diagnostics_tests.rs`](file:///d:/FlowGuardAG/tests/diagnostics_tests.rs) | `test_01_*` through `test_05_*` |
| **Domain mapper unit tests (30 tests)** | [`tests/domain_mapper_tests.rs`](file:///d:/FlowGuardAG/tests/domain_mapper_tests.rs) | `test_freshness_*`, `test_map_*`, `test_01_*` - `test_10_*` |
| **Mock integration tests (24 scenarios)** | [`tests/mock_scenarios.rs`](file:///d:/FlowGuardAG/tests/mock_scenarios.rs) | `test_01_*` through `test_24_*` |
| **Unicode truncation regression tests (15 tests)** | [`tests/unicode_truncation_tests.rs`](file:///d:/FlowGuardAG/tests/unicode_truncation_tests.rs) | `test_01_*` through `test_15_*` |
| **Live ActivityWatch integration test** | [`tests/real_integration_test.rs`](file:///d:/FlowGuardAG/tests/real_integration_test.rs) | `test_real_activitywatch_integration_read_only` |

---

## 2. Directory Responsibilities

### `src/domain/`
Contains pure business logic and models. Has zero knowledge of HTTP, reqwest, ActivityWatch APIs, or Windows handles.
- `activity.rs`: `ActivityObservation`, `BrowserContext`, `IdleState`, `FreshnessState`.
- `capabilities.rs`: `ActivityCapabilities`, `SourceCapabilities`.
- `diagnostics.rs`: `DiagnosticReport`, `SubsystemDiagnostic`, `SubsystemStatus`.
- `source_health.rs`: `SourceHealth` state machine.

### `src/application/`
Orchestrates workflow and defines integration ports.
- `ports.rs`: Defines the `ActivitySource` trait (`connect`, `poll`, `health`, `diagnose`) and `SourceError` error taxonomy.
- `activity_service.rs`: Coordinates polling cycles, monitors state changes, and delegates doctor diagnostics to `source.diagnose()`.

### `src/infrastructure/`
Contains external service adapters.
- `activitywatch/client.rs`: Reusable HTTP client communicating with ActivityWatch REST endpoints.
- `activitywatch/discovery.rs`: Resolves bucket names and IDs dynamically with strict ambiguity protection.
- `activitywatch/mapper.rs`: Transforms ActivityWatch events into normalized domain objects via `WatcherResult` without mutating structural capabilities.
- `activitywatch/models.rs`: Deserialization structs for ActivityWatch API JSON responses.
- `activitywatch/mod.rs`: Assembles components into `ActivityWatchAdapter`, implementing `ActivitySource`.

### `src/presentation/`
Translates domain models into terminal user interface views.
- `console.rs`: Formats `doctor` diagnostics, `status` badges, `observe --once` cards, and compact streaming logs.
- `truncate.rs`: Reusable panic-free Unicode-aware truncation utilities (`truncate_str`, `truncate_to_width`, `truncate_to_graphemes`) respecting grapheme clusters and terminal column widths.

### `src/config.rs`
Loads, parses, and validates user configuration files from candidate locations (%APPDATA%, local directory, or CLI flags).
