# ADR-001: External ActivityWatch REST API Integration

## Status
Accepted

## Context
FlowGuard needs an activity observation pipeline to monitor active desktop applications, window titles, user idle state, and web browser context. Developing a native Windows low-level monitoring system from scratch in Milestone 1 would divert focus from building FlowGuard's core workflow and productivity abstractions. ActivityWatch is an established, open-source, local-first activity tracker.

## Decision
In Milestone 1, FlowGuard integrates with ActivityWatch strictly as an **external service** via its documented local REST API (`http://127.0.0.1:5600`).

FlowGuard shall:
1. Connect via HTTP to ActivityWatch's local REST endpoints (`/api/0/info`, `/api/0/buckets/`, `/api/0/buckets/{id}/events`).
2. Require the user to install and run ActivityWatch manually.
3. Reject endpoints that do not validate as ActivityWatch during handshake.

FlowGuard shall NOT:
1. Fork or embed ActivityWatch source code.
2. Directly open or query ActivityWatch's SQLite database files.
3. Start, stop, install, or manage ActivityWatch background processes.
4. Modify ActivityWatch configuration files or datastores.
5. Write events or create buckets in ActivityWatch.

## Consequences
- **Positive**: Rapid foundation for FlowGuard with zero low-level Win32 hook complexity in M1.
- **Positive**: Strict decoupling protects FlowGuard from external implementation details.
- **Negative**: User must install and run ActivityWatch separately for M1.
- **Mitigation**: If ActivityWatch is unreachable, FlowGuard clearly reports disconnection and guidance rather than crashing or pretending to monitor.
