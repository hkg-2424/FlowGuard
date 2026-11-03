# ADR-002: Replaceable ActivitySource Boundary Abstraction

## Status
Accepted

## Context
While FlowGuard uses ActivityWatch in Milestone 1, the long-term vision (Milestone 7) requires an independent, native Rust monitoring implementation (`NativeWindowsAdapter`) that eliminates external dependencies. If the domain layer or workflow engine interacts directly with ActivityWatch concepts (bucket IDs, REST paths, watcher names, or event schemas), migrating to a native implementation would require widespread refactoring.

## Decision
Create an architectural boundary trait `ActivitySource` in the application layer:

```rust
pub trait ActivitySource: Send {
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError>;
    fn poll(&mut self) -> Result<ActivityObservation, SourceError>;
    fn health(&self) -> SourceHealth;
}
```

Rules:
1. All downstream layers (Workflow Engine, Analytics, Intervention Engine, CLI/UI) interact **exclusively** with `ActivitySource` and the normalized `ActivityObservation` domain model.
2. The domain model must contain zero ActivityWatch-specific fields (e.g. no bucket IDs, no heartbeat durations, no AW-specific event strings).
3. The ActivityWatch implementation is encapsulated entirely within `ActivityWatchAdapter`.
4. In Milestone 7, `NativeWindowsAdapter` will implement this identical trait using Win32 API calls (`GetForegroundWindow`, `GetWindowTextW`, `QueryFullProcessImageNameW`, `GetLastInputInfo`), allowing a seamless drop-in replacement via configuration (`provider = "native_windows"`).

## Consequences
- **Positive**: Clean separation of concerns following SOLID principles.
- **Positive**: Future native Windows implementation requires zero changes to the core domain, workflow engine, or UI.
- **Positive**: Simplifies automated testing by allowing mock adapters.

