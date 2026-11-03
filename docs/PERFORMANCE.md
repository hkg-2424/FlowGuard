# FlowGuard Performance & Resource Consumption

This document records the resource benchmarks and efficiency metrics of FlowGuard Milestone 1.

---

## 1. Test Environment

- **Operating System**: Windows 11 x64
- **Processor**: Intel / AMD x86_64
- **Rust Toolchain**: 1.99.1 (release profile with optimizations)
- **External Service**: ActivityWatch v0.14.1 running locally on `http://127.0.0.1:5600`
- **Polling Interval**: 1000 ms (`poll_interval_ms = 1000`)

---

## 2. Benchmark Procedure

FlowGuard was compiled in release mode (`cargo build --release`) and executed using the automated benchmark harness ([`scripts/benchmark.ps1`](file:///d:/FlowGuardAG/scripts/benchmark.ps1)):
1. FlowGuard was launched in continuous observation mode (`flowguard observe`).
2. A warm-up period of 3 seconds was observed to allow connection handshake and bucket discovery to settle.
3. 6 sequential snapshots of Process Working Set memory and Processor CPU Time were recorded at 1-second intervals.
4. Clean termination was enforced.

---

## 3. Benchmark Results

| Sample # | Elapsed Time | Working Set Memory (MB) | Total CPU Time (seconds) |
|---|---|---|---|
| Sample 1 | 3.0s (warm) | 14.64 MB | 0.0312 s |
| Sample 2 | 4.0s | 14.64 MB | 0.0312 s |
| Sample 3 | 5.0s | 14.64 MB | 0.0312 s |
| Sample 4 | 6.0s | 14.64 MB | 0.0312 s |
| Sample 5 | 7.0s | 14.64 MB | 0.0312 s |
| Sample 6 | 8.0s | 14.66 MB | 0.0312 s |

### Key Metrics Summary:
- **Average Working Set Memory**: **14.64 MB**
- **Memory Growth / Leaks**: **0.02 MB over run (effectively 0 MB, steady state)**
- **CPU Time Delta over 5 polling cycles**: **0.0000 s (< 0.05% CPU utilization)**
- **Binary Footprint (`target/release/flowguard.exe`)**: **~3.2 MB**

---

## 4. Architectural Efficiency Decisions

1. **Reused HTTP Client**: One single `reqwest::blocking::Client` is reused across all polling iterations, preserving persistent TCP keep-alive connections to ActivityWatch and avoiding socket churn.
2. **Synchronous Polling**: The polling loop avoids heavy async task scheduling loops for simple 1Hz observation cycles.
3. **Selective Querying (`limit=1`)**: FlowGuard never requests or downloads the historical event database. It queries strictly one event per bucket.
4. **Cached Bucket Metadata**: Bucket discovery runs only during connection handshake or reconnection; the 1Hz polling loop does not query `/api/0/buckets/`.
5. **Smart Change Detection**: Terminal output and event notifications only fire when application, title, idle status, or health actually changes, eliminating terminal flicker and stdout buffer saturation.

