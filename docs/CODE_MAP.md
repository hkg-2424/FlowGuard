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
| **Event mapping & freshness calculations** | [`src/infrastructure/activitywatch/mapper.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/mapper.rs) | `check_freshness`, `map_events_to_observation` |
| **ActivityWatch adapter implementation** | [`src/infrastructure/activitywatch/mod.rs`](file:///d:/FlowGuardAG/src/infrastructure/activitywatch/mod.rs) | `ActivityWatchAdapter` |
| **Source abstraction interface (Trait)** | [`src/application/ports.rs`](file:///d:/FlowGuardAG/src/application/ports.rs) | `pub trait ActivitySource`, `SourceError` |
| **Normalized activity domain model** | [`src/domain/activity.rs`](file:///d:/FlowGuardAG/src/domain/activity.rs) | `ActivityObservation`, `BrowserContext`, `IdleState` |
| **Capability definitions** | [`src/domain/capabilities.rs`](file:///d:/FlowGuardAG/src/domain/capabilities.rs) | `ActivityCapabilities`, `SourceCapabilities` |
| **Source health definitions** | [`src/domain/source_health.rs`](file:///d:/FlowGuardAG/src/domain/source_health.rs) | `SourceHealth` |
| **Polling orchestration & change detection** | [`src/application/activity_service.rs`](file:///d:/FlowGuardAG/src/application/activity_service.rs) | `ActivityService::observe`, `run_doctor` |
| **CLI command implementations & output format** | [`src/presentation/console.rs`](file:///d:/FlowGuardAG/src/presentation/console.rs) | `print_doctor_report`, `print_observation_change` |
| **CLI arguments & argument parsing** | [`src/main.rs`](file:///d:/FlowGuardAG/src/main.rs) | `struct Cli`, `enum Commands` |
| **Configuration schema & validation** | [`src/config.rs`](file:///d:/FlowGuardAG/src/config.rs) | `FlowGuardConfig`, `ActivityWatchConfig` |
| **Example configuration file** | [`config/flowguard.example.toml`](file:///d:/FlowGuardAG/config/flowguard.example.toml) | TOML template |
| **Mock integration tests (18 scenarios)** | [`tests/mock_scenarios.rs`](file:///d:/FlowGuardAG/tests/mock_scenarios.rs) | `test_01_*` through `test_18_*` |
| **Domain mapper unit tests** | [`tests/domain_mapper_tests.rs`](file:///d:/FlowGuardAG/tests/domain_mapper_tests.rs) | `test_freshness_*`, `test_map_*` |
| **Live ActivityWatch integration test** | [`tests/real_integration_test.rs`](file:///d:/FlowGuardAG/tests/real_integration_test.rs) | `test_real_activitywatch_integration_read_only` |

---

## 2. Directory Responsibilities

### `src/domain/`
Contains pure business logic and models. Has zero knowledge of HTTP, reqwest, ActivityWatch APIs, or Windows handles.
- `activity.rs`: `ActivityObservation`, `BrowserContext`, `IdleState`, `FreshnessState`.
- `capabilities.rs`: `ActivityCapabilities`, `SourceCapabilities`.
- `source_health.rs`: `SourceHealth` state machine.

### `src/application/`
Orchestrates workflow and defines integration ports.
- `ports.rs`: Defines the `ActivitySource` trait and `SourceError` error taxonomy.
- `activity_service.rs`: Coordinates polling cycles, monitors state changes, and runs the diagnostic doctor.

### `src/infrastructure/`
Contains external service adapters.
- `activitywatch/client.rs`: Reusable HTTP client communicating with ActivityWatch REST endpoints.
- `activitywatch/discovery.rs`: Resolves bucket names and IDs dynamically.
- `activitywatch/mapper.rs`: Transforms ActivityWatch events into normalized domain objects and computes freshness.
- `activitywatch/models.rs`: Deserialization structs for ActivityWatch API JSON responses.
- `activitywatch/mod.rs`: Assembles components into `ActivityWatchAdapter`, implementing `ActivitySource`.

### `src/presentation/`
Translates domain models into terminal user interface views.
- `console.rs`: Formats `doctor` diagnostics, `status` badges, `observe --once` cards, and compact streaming logs.

### `src/config.rs`
Loads, parses, and validates user configuration files from candidate locations (%APPDATA%, local directory, or CLI flags).
