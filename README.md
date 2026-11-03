# FlowGuard

> **Local-first Windows focus and productivity assistant.**

FlowGuard helps users maintain their intended workflow rather than continually fragmenting their attention through unrelated context-switching. Instead of treating every application switch as a distraction, FlowGuard understands workflows across IDEs, browsers, resources, and reference documentation.

---

## Milestone 1: ActivityWatch Integration Foundation

This milestone delivers the foundation of FlowGuard: a clean, reliable, and decoupled activity observation layer using an existing local [ActivityWatch](https://activitywatch.net/) installation via its REST API.

FlowGuard communicates exclusively with the external ActivityWatch local REST API (`http://127.0.0.1:5600`). It translates external telemetry into FlowGuard's clean internal domain models through a strictly defined `ActivitySource` trait, ensuring that future native monitoring (e.g., Win32 API observers) can replace ActivityWatch with zero changes to downstream engines.

### Key Capabilities in M1

- **Replaceable `ActivitySource` Abstraction**: Pure domain models (`ActivityObservation`, `BrowserContext`, `IdleState`, `ActivityCapabilities`) completely decoupled from ActivityWatch internals.
- **Robust Identity & Port Verification**: Validates the service signature against `/api/0/info`. Detects and cleanly flags port collisions if another application occupies port 5600.
- **Dynamic Bucket Discovery**: Discovers buckets dynamically by type (`currentwindow`, `afkstatus`, `web.tab.current`) and local hostname without hardcoding bucket IDs.
- **Independent Staleness Tracking**: Detects stopped watchers and flags observations as `Stale` (configurable threshold, default 15s) using timestamp and heartbeat duration arithmetic. Never fabricates activity or mistakes missing data for active work.
- **Privacy by Design**: Runs 100% locally. Zero telemetry, zero network egress, zero keylogging, zero screenshots, and sanitizes window titles and query parameters in logs.
- **Ultra-lightweight**: Average memory footprint of ~14.6 MB working set and < 0.05% CPU usage during 1000ms polling.

---

## Quick Start

### Prerequisites

- Windows 11 x64 (or Windows 10 x64)
- Rust stable (1.80+) & Cargo
- [ActivityWatch](https://activitywatch.net/) installed and running locally

### Build

```powershell
# Build in debug mode
cargo build

# Build in release mode
cargo build --release
```

### CLI Commands

```powershell
# 1. Integration & Watcher Diagnostics
.\target\release\flowguard.exe doctor

# 2. Check Connection & Capability Status
.\target\release\flowguard.exe status

# 3. Read a Single Normalized Snapshot
.\target\release\flowguard.exe observe --once

# 4. Continuous Activity Stream (only logs meaningful changes)
.\target\release\flowguard.exe observe
```

### Optional Flags

- `--config <path>`: Load a custom TOML configuration file.
- `--verbose`: Enable detailed output (includes full window titles, raw URLs, and debug traces).

---

## Project Structure

```text
flowguard/
├── Cargo.toml
├── README.md
├── CHANGELOG.md
├── config/
│   └── flowguard.example.toml
├── docs/
│   ├── ARCHITECTURE.md
│   ├── ACTIVITYWATCH_INTEGRATION.md
│   ├── CODE_MAP.md
│   ├── PRODUCT_VISION.md
│   ├── ROADMAP.md
│   ├── PRIVACY.md
│   ├── TESTING.md
│   ├── PERFORMANCE.md
│   ├── MILESTONE_1_ACCEPTANCE.md
│   ├── PROJECT_STATUS.md
│   └── decisions/
│       ├── ADR-001-activitywatch-external-integration.md
│       ├── ADR-002-replaceable-activity-source.md
│       └── ADR-003-activity-data-privacy.md
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── config.rs
│   ├── domain/
│   ├── application/
│   ├── infrastructure/
│   └── presentation/
└── tests/
    ├── domain_mapper_tests.rs
    ├── mock_scenarios.rs
    └── real_integration_test.rs
```

For a comprehensive guide on navigating and editing the codebase, see [docs/CODE_MAP.md](docs/CODE_MAP.md).
