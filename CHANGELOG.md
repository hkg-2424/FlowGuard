# Changelog

All notable changes to FlowGuard will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-03

### Added
- **Milestone 1 — ActivityWatch Integration Foundation**:
  - `ActivitySource` trait in `application/ports.rs` providing an activity observation contract for ActivityWatch and future native Windows monitoring.
  - Domain models: `ActivityObservation`, `BrowserContext`, `IdleState`, `FreshnessState`, `ActivityCapabilities`, and `SourceHealth`.
  - Reusable HTTP client for ActivityWatch in `infrastructure/activitywatch/client.rs` supporting endpoint normalization, custom ports, request timeouts, and optional Bearer token authentication via environment variables.
  - Strict handshake verification via `GET /api/0/info` with explicit `WrongService` handling when port 5600 is occupied by an unrelated service.
  - Dynamic bucket discovery in `infrastructure/activitywatch/discovery.rs` prioritizing local hostname matching, detecting bucket ambiguities, and supporting optional configuration overrides.
  - Independent freshness engine in `infrastructure/activitywatch/mapper.rs` calculating event age using timestamps and heartbeat durations; treats events older than staleness threshold (default 15s) as `Stale`.
  - Continuous observation service with change detection in `application/activity_service.rs` and bounded reconnect backoff.
  - CLI commands implemented via `clap`:
    - `flowguard doctor`: Deep diagnostic inspection of endpoints, handshake metadata, bucket status, and watcher staleness.
    - `flowguard status`: Concise summary of operational health and detected capabilities.
    - `flowguard observe --once`: Snapshot display of current normalized user activity.
    - `flowguard observe`: Streaming real-time activity change monitor with graceful Ctrl+C handling.
  - Privacy safeguards: Redacts sensitive query parameters from URLs and truncates long window titles by default.
  - Comprehensive test suite:
    - 9 domain mapper unit tests.
    - 18 mock HTTP integration tests covering edge cases, timeouts, port conflicts, auth errors, and reconnects.
    - 1 read-only live integration test.
  - Documentation suite including Architecture, Code Map, Integration Guide, Privacy Guarantee, Benchmarks, Acceptance Criteria, and ADRs (001-003).
