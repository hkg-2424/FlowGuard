# ADR-003: Activity Data Privacy and Local-First Processing

## Status
Accepted

## Context
Monitoring application usage, active windows, and browsing tabs involves sensitive personal data. Many productivity trackers upload raw screenshots, keystroke logs, or full browser history to cloud servers, introducing severe security, privacy, and compliance liabilities.

## Decision
FlowGuard adheres to a strict **100% local-first data processing** architecture:

1. **No External Egress**: FlowGuard will never upload activity events, telemetry, or analytics to remote cloud services.
2. **Minimal Data Ingestion**: FlowGuard reads only active application names, window titles, user idle state, and active tab context from ActivityWatch.
3. **No Invasive Monitoring**: FlowGuard does not take screenshots, record keystrokes, log mouse coordinates, inspect clipboard contents, or read disk files.
4. **Log Sanitization**: Full URLs and window titles are sanitized in standard console outputs to prevent accidental leakage of sensitive tokens, query parameters, or private document paths.
5. **No Secondary Data Store**: Raw events from ActivityWatch are not duplicated into a separate database during M1.

## Consequences
- **Positive**: Complete user trust, zero risk of remote data breaches, and zero external compliance burden.
- **Negative**: Syncing across multiple machines requires future local-network or encrypted peer-to-peer mechanisms rather than simple cloud logins.

