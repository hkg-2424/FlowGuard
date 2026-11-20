# FlowGuard Comprehensive Code Review

**Review date:** 2026-11-20  
**Reviewed revision:** `5152fc2` on `main`  
**Method:** Static, read-only review of the current repository  
**Source changes made:** None

## 1. Executive Summary

This review evaluates the current FlowGuard implementation as a local-first Windows focus assistant. The project has a clear layered structure: normalized activity acquisition, a deterministic workflow engine, persistence adapters, and an embedded local web interface. It also has useful regression coverage around ActivityWatch mapping, Unicode handling, workflow evaluation, SQLite, and the GUI server.

The current implementation is not release-ready for high-trust or "unbreakable" use. The most important risks are in session lifecycle atomicity, policy/timer semantics, local GUI authorization, time accounting, and persistence error handling. This catalogue records 2 critical, 32 high, 40 medium, and 5 low implementation findings, plus 6 release/supply-chain findings. Several documents also describe a future architecture or previously fixed state rather than the code that is currently present.

Findings below distinguish:

- **Confirmed defect:** A concrete execution path produces incorrect or unsafe behavior.
- **Risk:** The implementation has a credible failure or security mode that needs validation.
- **Specification gap:** The behavior differs from the documented product contract.
- **Missing feature/test:** The behavior is absent or insufficiently verified, but is not necessarily a defect in the implemented milestone.

## 2. Scope And Method

The review covered all 84 Git-tracked files plus the ignored `Cargo.lock` and root `FlowGuard.exe` artifacts:

- Rust source under `src/`.
- Browser UI assets under `src/presentation/gui/static/`.
- All integration tests under `tests/`.
- Project configuration, example configuration, benchmark script, and dependency lock data.
- All documents and architecture decision records under `docs/`.
- Existing historical reviews under `code_reviews/`.
- The ignored root executable and tracked `versions/v1/flowguard.exe` as opaque release artifacts.

The `.git/` database and generated build output are repository metadata/artifacts, not application source, and were not treated as reviewable product code. The executable files were inspected for metadata and provenance but were not executed or reverse-engineered.

To honor the no-modification constraint, no build, test, migration, server, or executable was run. Such commands can create or update `target/`, `Cargo.lock`, databases, and runtime state. `cargo fmt --all -- --check` and `git diff --check` were run because they are read-only checks; both passed. Findings are therefore based on static execution-path analysis rather than runtime confirmation.

## 3. Current Architecture

| Layer | Current responsibility |
|---|---|
| `src/domain/` | Activity models, health/capabilities, workflow entities, rule decisions, session state, and grace timer logic. |
| `src/application/` | Activity polling and workflow/session orchestration through repository and source ports. |
| `src/infrastructure/activitywatch/` | ActivityWatch HTTP client, bucket discovery, event DTOs, and normalization. |
| `src/infrastructure/native_windows/` | Native foreground-window and idle-state acquisition. |
| `src/infrastructure/auto_source.rs` | Primary/fallback activity-source selection. |
| `src/infrastructure/persistence/` | In-memory and SQLite workflow/session repositories. |
| `src/presentation/` | CLI output, session runner, embedded GUI server/engine/state, and static browser UI. |
| `src/main.rs` | Command-line parsing and dependency composition. |

The intended dependency direction is generally sound: domain code does not directly depend on HTTP, Win32, SQLite, or UI code. Most serious problems are at orchestration boundaries where state is mutated in memory, persisted separately, or exposed through the unauthenticated local GUI.

## 4. Findings

Findings are ordered by severity. Line references identify the current reviewed revision and may move after future edits.

### Critical

#### C-01: The loopback GUI API is unauthenticated and cross-origin accessible

**Type:** Confirmed security/privacy defect  
**Evidence:** `src/presentation/gui/server.rs:65-138,272-447,489-507`

Every JSON response sends `Access-Control-Allow-Origin: *`; preflight permits JSON POSTs; and requests require no capability token, cookie, `Origin`, `Referer`, or `Host` validation. The port is predictable and arbitrary Host headers permit DNS-rebinding scenarios where browser Local Network Access protections do not intervene. A hostile site, extension, or local process can read visible window titles, live activity, history, reflection reasons, and drift targets; it can also create/start a very long unbreakable workflow or pause/resume/stop sessions. A bodyless simple POST can stop a Standard session because an invalid/empty body becomes `reason: None`.

Bind-to-loopback is not authentication. Require a random per-process capability on every request, validate the exact loopback Host and expected Origin, remove wildcard CORS, reject untrusted origins, require a non-simple CSRF header, and consider named-pipe/WebView IPC instead of a general HTTP control plane.

#### C-02: Public completion bypasses commitment contracts and target duration

**Type:** Confirmed core-contract defect  
**Evidence:** `src/domain/workflow/session.rs:138-148`, `src/application/workflow_service.rs:181-196`, `tests/workflow_engine_tests.rs:445-463`, `tests/workflow_service_tests.rs:261-276`

An Accountable or Unbreakable session can be started and immediately ended through `complete()`/`complete_session()` without reaching its target or providing a reason. Existing tests explicitly perform and accept this transition, so the supposed protection is not only untested but normalized. Enforce the authoritative monotonic target inside the domain transition, or make completion a capability-protected scheduler operation that callers cannot invoke early.

### High

#### H-01: Configuration can violate zero-egress guarantees and disclose environment secrets

**Type:** Confirmed security/design defect  
**Evidence:** `src/config.rs:98-103,172-190`, `src/infrastructure/activitywatch/client.rs:34-42,78-98`

Configuration validation checks only whether `base_url` starts with `http://` or `https://`. It permits remote hosts, URL userinfo, query/fragment components, and confusing strings such as `http://127.0.0.1:5600@attacker.example`. FlowGuard implicitly loads a working-directory configuration, and `api_key_env` can name any environment variable. A planted configuration can therefore select `https://attacker.example`, name a sensitive variable such as `AWS_SECRET_ACCESS_KEY`, and transmit its value as a bearer token during the first handshake.

This enables external egress, internal-network probing, and secret disclosure in direct conflict with `README.md:13,21`. Parse the URL structurally; require literal loopback addresses by default; reject userinfo/query/fragment; disable redirects and environment proxies for loopback IPC; constrain secret selection; and require a conspicuous opt-in for remote servers.

#### H-02: Bucket discovery can select activity from another machine

**Type:** Confirmed correctness/privacy defect  
**Evidence:** `src/infrastructure/activitywatch/discovery.rs:190-205`

When no bucket matches the ActivityWatch server hostname, discovery falls back to all matching buckets. A database containing exactly one imported or synchronized `currentwindow` bucket for another host is therefore accepted as the local foreground source. That device's titles and browser activity can drive policy decisions. Automatic selection must require a same-host match; foreign buckets should require an explicit override.

#### H-03: Watcher discovery and capability state can remain stale until restart

**Type:** Confirmed lifecycle defect  
**Evidence:** `src/domain/source_health.rs:19-23`, `src/infrastructure/activitywatch/mod.rs:156-177,223-267`

`Degraded` is considered operational, so missing watchers discovered at startup are never searched for again. Clearing a missing bucket before a one-shot rediscovery can also leave no future rediscovery trigger, while cached capability flags remain true. Starting a browser/AFK/window watcher later or temporarily failing rediscovery can permanently remove or falsely advertise channels. Track discovery expiry explicitly, retry with backoff, update discovery atomically, and recompute capabilities whenever a channel changes.

#### H-04: Automatic fallback never recovers automatically to ActivityWatch

**Type:** Confirmed availability/capability defect  
**Evidence:** `src/infrastructure/auto_source.rs:86-121`, `src/config.rs:56-57`

After switching to the native adapter, `poll()` only polls native Windows and never probes ActivityWatch. A transient outage permanently removes browser URL/incognito capabilities until a manual reconnect, diagnostic call, or restart. The configured reconnect interval is unused here. Add periodic recovery probes, hysteresis, and a health gate before switching back.

#### H-05: Fallback hides authentication, wrong-service, and configuration failures

**Type:** Confirmed diagnostics/policy defect  
**Evidence:** `src/infrastructure/auto_source.rs:63-82,131-163`, `src/infrastructure/mod.rs:14-36`

`connect()` falls back on every primary error, not only transient unavailability; `diagnose()` replaces authentication or wrong-service evidence with a generic "not running" message. Initial `poll()` uses a different error policy. As a result, a port collision, expired credential, or incompatible service can be reported as a healthy native fallback. Preserve the primary report and make strict versus automatic provider behavior explicit and consistent.

#### H-06: Native Windows health and capabilities are asserted without successful observations

**Type:** Confirmed health-model defect  
**Evidence:** `src/infrastructure/native_windows/mod.rs:45-53,63-101,108-157`, `src/infrastructure/native_windows/api.rs:315-332`

The adapter always connects as healthy, marks window freshness `Fresh` even when application and title are both absent, and keeps overall health connected when idle acquisition fails. Non-Windows stubs return no useful data but still advertise Win32 capabilities and health. This makes auto fallback appear successful on lock screens, protected windows, API failure, and unsupported targets. Gate the provider to Windows and derive freshness, capabilities, subsystem status, and overall health from actual probes.

#### H-07: Cached diagnostics can certify a stopped or replaced server

**Type:** Confirmed diagnostics defect  
**Evidence:** `src/infrastructure/activitywatch/mod.rs:289-319,325-546,554-567`

Once connected, `diagnose()` can skip a new handshake because both `Connected` and `Degraded` are considered operational. If the server subsequently stops or is replaced, channel queries can all fail while the report retains `is_reachable: true`, ActivityWatch identity, and cached metadata. It also does not assign calculated overall health back to the adapter. Doctor must always perform a fresh identity probe and update adapter state.

#### H-08: Evaluation persistence silently loses data and is non-atomic

**Type:** Confirmed data-integrity defect  
**Evidence:** `src/application/workflow_service.rs:242-265,308`, `src/application/ports.rs:135-144`

Both drift-event insertion and the final session save discard repository errors. They are separate commits, so either can succeed alone; disk-full, corruption, and lock failures still produce a successful `SessionEvaluation`. History and aggregate counters can permanently disagree without a warning. Persist each evaluation through one repository unit-of-work/SQLite transaction and surface or queue failed writes with an explicit storage-degraded state.

#### H-09: Repository failures leave lifecycle state irrecoverably inconsistent

**Type:** Confirmed state/data-loss defect  
**Evidence:** `src/application/workflow_service.rs:127-151,160-193,213-224`

Pause/resume mutate memory and timers before saving. Cancel/complete remove the active session and reset timing state before saving. If persistence fails, the former operations return an error even though memory changed, while the latter lose the only in-memory session and leave SQLite active, making retry impossible. Paused ticks also return before periodic persistence. Apply transitions to a clone, persist first, then atomically install/clear state; retain a retryable dirty state when storage is unavailable.

#### H-10: Millisecond session IDs can collide and produce hybrid rows

**Type:** Confirmed identity/data-corruption defect  
**Evidence:** `src/application/workflow_service.rs:102`, `src/infrastructure/persistence/sqlite/session_repository.rs:153-180`

Two processes starting in one millisecond, or a clock rollback, can generate the same ID. The SQLite conflict update changes state/counters but not workflow, contract, or start time, combining immutable values from session A with mutable values from B. Use random/monotonic globally unique IDs; distinguish creation from update; reject duplicate inserts; and add update versioning.

#### H-11: Corrupt stored values are silently converted into valid-looking state

**Type:** Confirmed corruption-handling defect  
**Evidence:** `src/infrastructure/persistence/sqlite/workflow_repository.rs:75-100,180-194`, `src/infrastructure/persistence/sqlite/session_repository.rs:96-115,244-249,337-356`

Unknown scopes/effects/states and malformed timestamps are mapped to fallback enums, `Utc::now()`, or `None`. An invalid expiry can turn a temporary allow rule into a permanent one, and a corrupt historical time changes on every read. Every invalid stored enum/time must return a typed corrupt-data error; database contents must never be silently reinterpreted.

#### H-12: Crash recovery writes unreadable timestamps and can interrupt another live instance

**Type:** Confirmed lifecycle/multi-process defect  
**Evidence:** `src/presentation/gui/mod.rs:51-56`, `src/infrastructure/persistence/sqlite/session_repository.rs:113-115`

GUI startup updates every active/paused row and writes SQLite `datetime('now')`, while the loader accepts only RFC3339; recovered interrupted sessions therefore have no end time. Because no process ownership/lease is checked, a second live GUI can mark the first GUI's session interrupted, after which the first may write it active again. Recovery belongs in a repository transaction using RFC3339, a process lock or lease, and owner-liveness validation.

#### H-13: The one-active-session invariant exists only inside one service object

**Type:** Confirmed concurrency defect  
**Evidence:** `src/application/workflow_service.rs:89-95`, `src/infrastructure/persistence/sqlite/schema.rs:34-52`, `src/application/ports.rs:127-150`

There is no database constraint, lease, or query preventing multiple processes/services from starting simultaneous sessions. This creates competing enforcement, lost updates, and lifecycle oscillation. Use a single-instance process lock or transactional database lease; if global uniqueness is required, enforce it with an appropriate partial unique constraint and ownership protocol.

#### H-14: The GUI server permits unbounded-body/thread denial of service

**Type:** Confirmed availability defect  
**Evidence:** `src/presentation/gui/server.rs:37-43,139-150,354-367,414-419`

Every accepted request spawns a detached OS thread, while bodies are read into an unbounded `String` with no content-length limit, read deadline, concurrency limit, or rate limit. Large or slow local requests can exhaust memory/threads and block SQLite-backed GUI work. Use a bounded worker model, strict body/field/list limits, deadlines, and rate/concurrency controls.

#### H-15: Multiple DOM-XSS paths execute untrusted workflow and activity text

**Type:** Confirmed security defect  
**Evidence:** `src/presentation/gui/static/app.js:307-316,416-430,559-594,625-670`

`showToast()` inserts messages containing workflow names, domains, server errors, display names, and titles through `innerHTML`. Generated inline event attributes rely on an `escapeJs()` routine that leaves HTML entities such as `&#39;` able to decode into a quote after parsing. A crafted workflow/window title can execute script in the privileged local origin, read all API data, mutate sessions, and exfiltrate history. Build DOM nodes, assign untrusted values with `textContent`, use `addEventListener`, and enforce a strict CSP after removing inline code.

#### H-16: The API and state model expose substantially more private data than the UI needs

**Type:** Confirmed privacy defect  
**Evidence:** `src/presentation/gui/state.rs:34-60`, `src/presentation/gui/server.rs:84-123,272-347`, `src/presentation/gui/static/app.js:980-1071`

Responses contain full titles, 50 activity entries, all visible application titles, early-exit reflections, and raw drift targets; passive collection continues without an active session. Sensitive APIs also omit `Cache-Control: no-store`. Minimize per-view fields, canonicalize/redact before both state and persistence, add retention/privacy controls, authenticate reads, and prohibit caching.

#### H-17: FlowGuard's own GUI is normally treated as a workflow violation

**Type:** Confirmed product defect  
**Evidence:** `src/presentation/gui/mod.rs:43-44`, `src/presentation/gui/static/index.html:6`, `src/presentation/gui/seed.rs:18-289`, `tests/desktop_gui_tests.rs:203-206`

The control UI runs in Edge/Chrome at a localhost origin, but default workflows allow neither that origin nor a neutral FlowGuard control surface. Starting a session while the GUI remains foreground ordinarily begins grace; bringing an intervention UI forward can perpetuate the violation. The GUI test masks this with a `code.exe` observation. Identify the owned FlowGuard origin/window as neutral: it should neither earn focus nor consume grace.

#### H-18: Empty workflow selections deny everything despite UI text saying "allow all"

**Type:** Confirmed UI/domain mismatch  
**Evidence:** `src/presentation/gui/static/app.js:559-583`, `src/presentation/gui/server.rs:183-248`, `src/domain/workflow/entities.rs:160-174`

With no apps/sites selected, the server adds no allow rule while retaining a restrictive workflow default. A novice therefore creates an immediate deny-all workflow. Define each empty dimension explicitly by adding appropriate wildcard semantics or require a selection and correct the UI copy.

#### H-19: Completion and commitment duration use inconsistent, incorrect clocks

**Type:** Confirmed core-product defect  
**Evidence:** `src/presentation/gui/engine.rs:148-176`, `src/presentation/gui/state.rs:358-364`, `src/presentation/session_runner.rs:250-271`, `src/presentation/gui/static/app.js:764-773`

GUI completion uses focus + grace + distraction + paused time, so a paused or fully distracted session can finish. CLI completion uses wall time, so AFK/source outage counts; Unbreakable cancellation also has a wall-clock fallback. UI labels imply AFK/timers pause and calls the aggregate "Elapsed Focus." Define one authoritative elapsed metric (for example active elapsed excluding pause/AFK) and use it in GUI, CLI, snapshots, and contracts.

#### H-20: Timed-out HTTP commands can mutate state later

**Type:** Confirmed asynchronous consistency defect  
**Evidence:** `src/presentation/gui/server.rs:369-445`, `src/presentation/gui/engine.rs:59-139`

The HTTP handler returns 504 after three seconds, but its command remains queued. It can start, pause, resume, or stop a session after the UI reported failure; a retry then races the original. Add command IDs and deadlines/cancellation, drop expired commands before mutation, and make operations idempotent.

#### H-21: Closing the advertised desktop window does not stop or finalize FlowGuard

**Type:** Confirmed lifecycle/privacy defect  
**Evidence:** `src/presentation/gui/mod.rs:115-136,166-183`, `src/presentation/gui/engine.rs:264-268`

The browser child handle is discarded and process lifetime depends on Ctrl+C. Closing the app-mode window leaves observation, HTTP control, alerts, and any active session running; graceful engine exit does not persist an interruption. Track the owned window/child, define deliberate tray/service behavior, provide authenticated shutdown, and finalize active state on every exit path.

#### H-22: CLI ad-hoc sessions overwrite a reserved GUI preset

**Type:** Confirmed cross-interface data-loss defect  
**Evidence:** `src/main.rs:245-338`, `src/presentation/gui/seed.rs:74-127`

The default CLI name maps to `wf-deep-work`, the same ID as the seeded GUI workflow. Saving the CLI workflow updates the preset and replaces all its rules, contract, and duration. Load the existing preset or use a unique ephemeral ID; never overwrite a reserved workflow without explicit update intent.

#### H-23: Terminal output permits control-sequence injection and leaks URL paths

**Type:** Confirmed presentation security/privacy defect  
**Evidence:** `src/presentation/console.rs:188-242,278-335`, `src/presentation/session_runner.rs:91-130,164-225`, `src/presentation/truncate.rs:53-67`, `src/main.rs:440-454`

Provider-controlled titles, diagnostics, targets, and reasons are printed without escaping ANSI/OSC, C0/C1, newline, or bidi controls; width truncation copies zero-width controls. URL sanitization removes only the query and retains userinfo, private paths, and fragments. Apply one terminal-safe escaping layer and parse URLs to a deliberately minimal normalized representation.

#### H-24: Uncertain observations both reset and continue grace depending on failure path

**Type:** Confirmed policy-safety defect  
**Evidence:** `src/domain/workflow/engine.rs:334-350`, `src/application/workflow_service.rs:227-232,330-338`

Stale/missing window/application data calls `timer.cancel()`, resetting consumed grace and potentially creating a false resolution. A source polling error never pauses the timer, so the same uncertainty can instead run it toward a false intervention. Use one uncertainty transition that pauses the episode and accounting without resolving it.

#### H-25: Browser and idle freshness are ignored during policy evaluation

**Type:** Confirmed stale-policy defect  
**Evidence:** `src/domain/workflow/engine.rs:334-383`

Only window freshness is gated. A stale browser URL can still allow/restrict a canonical resource or domain, while missing/stale idle information represented as Unknown is treated as active. Gate every matcher on the freshness/capability of its source and return an explicit reduced-confidence/unavailable decision where necessary.

#### H-26: Browser application gating uses disabled, expired, and restrictive domain rules incorrectly

**Type:** Confirmed rule-engine defect  
**Evidence:** `src/domain/workflow/engine.rs:449-479`

The presence of any active domain rule, including Restrict, changes an application rule into an allowlist. Its inner matching scan then includes disabled/expired domain rules. Chrome-Allow plus Reddit-Restrict can block unrelated sites, while an inactive Reddit pattern can help authorize Reddit when another active domain rule exists. Eliminate the shortcut or consider only explicitly active Allow rules with well-defined fallback semantics.

#### H-27: Hard-coded short-form media policy overrides explicit rules and matches unrelated activity

**Type:** Confirmed policy precedence defect  
**Evidence:** `src/domain/workflow/engine.rs:182-239,385-419`

Short-form detection runs before all workflow rules, making an ExactResource Allow for a specific Short unreachable. Raw substring checks also match unrelated domains/paths and non-browser research titles. Represent this restriction as an ordinary lower-specificity rule beneath explicit user exceptions and parse host/path boundaries.

#### H-28: Grace rule switches preserve the wrong deadline and can misclassify a late return

**Type:** Confirmed timer defect  
**Evidence:** `src/domain/workflow/timer.rs:67-107,119-137`, `src/domain/workflow/engine.rs:527-555`

An existing violation updates its rule/target but not `grace_duration`, while output reports the new duration. Switching from 30 seconds to custom zero therefore keeps the 30-second deadline but reports zero. If polling skips the expiry moment and the user returns after the deadline, `cancel()` still reports SelfCorrected because only an earlier tick can set `intervened`. Preserve episode start, apply an explicitly defined effective deadline (normally the stricter one), and evaluate expiry during cancellation.

#### H-29: Temporal accounting fabricates time and assigns intervals to the wrong state

**Type:** Confirmed metrics/completion defect  
**Evidence:** `src/application/workflow_service.rs:112,212-225,274-305`

Every zero/subsecond delta becomes at least one second; a 500 ms caller can count two seconds per wall second. Paused ticks add synthetic fixed time, and the interval before a context switch is assigned to the newly observed decision. Outage gaps can be charged wholesale to the first recovery state. Maintain fractional monotonic intervals classified by the prior state, split known timer boundaries, and record outage time as unaccounted.

#### H-30: Intervened drift history is delayed, cumulative, and lost on terminal transitions

**Type:** Confirmed analytics defect  
**Evidence:** `src/application/workflow_service.rs:229-268`

An intervention event is recorded only after a later Allowed observation. Its duration is session-wide grace plus distraction forced to at least 30 seconds, so a zero-grace one-second event becomes 30 and later episodes include earlier totals. Cancel/complete while violating records nothing. Track per-episode start/deadline/elapsed state and seal the event at intervention or session closure.

#### H-31: Intervention deduplication diverges from the timer state machine

**Type:** Confirmed counter defect  
**Evidence:** `src/domain/workflow/decision.rs:32-39`, `src/domain/workflow/engine.rs:364-375`, `src/application/workflow_service.rs:270-300`

AFK/system Ignored clears the service Boolean while the timer episode remains paused, so resuming one zero-grace episode increments again. Warning/Unavailable paths can cancel the timer without clearing the Boolean, suppressing the next real episode. Count edges from one authoritative timer episode identity/state rather than a parallel flag.

#### H-32: Title-based identity/domain matching is trivially spoofable

**Type:** Policy-bypass risk  
**Evidence:** `src/domain/workflow/engine.rs:130-160,302-307`

Generic untrusted title substrings can make `discord.exe` titled "code review" match Code, or an arbitrary web page titled "GitHub" satisfy a GitHub allow rule. Broad public-suffix-like patterns reduce to weak tokens. Never infer executable identity from a generic title; label title-only domain matching degraded/advisory and fail closed where strict enforcement is promised.

### Medium

#### M-01: Fresh timestamps with malformed payloads are classified as healthy

**Type:** Confirmed schema/health defect  
**Evidence:** `src/infrastructure/activitywatch/mapper.rs:321-330,379-390,435-470`

Payload deserialization failures fall back to default DTOs. Fresh `{}` window/AFK events can consequently yield fresh channels, no application/title, unknown idle state, connected health, and no schema diagnostic; malformed web events become an empty `BrowserContext`. Validate required fields independently from timestamp freshness and degrade the affected channel.

#### M-02: Connection health is based on bucket existence rather than usable events

**Type:** Specification gap  
**Evidence:** `src/domain/source_health.rs:9-12`, `src/infrastructure/activitywatch/mod.rs:123-133`

Finding two expected bucket IDs is enough to mark the source connected without checking whether they are queryable, nonempty, fresh, or schema-compatible. Either keep the source connecting/degraded until essential probes pass or separate transport connection from observation usability.

#### M-03: Configured reconnect backoff is bypassed on the main failure path

**Type:** Confirmed retry defect  
**Evidence:** `src/infrastructure/activitywatch/mod.rs:155-165,179-217`, `src/application/activity_service.rs:125-164`

Connection failures are commonly converted into a successful disconnected observation. `ActivityService` then sleeps for the normal poll interval rather than `reconnect_interval_secs`; a 100 ms poll setting can retry handshakes every 100 ms despite a 60-second reconnect setting. Apply one typed retry policy with capped backoff to transport and repeated server failures.

#### M-04: Polling cannot meet its documented 50 ms shutdown bound

**Type:** Confirmed responsiveness/documentation defect  
**Evidence:** `src/application/activity_service.rs:14-28,125-139`, `src/infrastructure/activitywatch/mod.rs:179-217`

Cancellation is checked around sleeps, not during serial blocking window, AFK, and web requests. At the default 1.5-second request timeout a connected poll can delay shutdown by roughly 4.5 seconds, longer if connection requests are included. Use a total poll deadline/cancellable I/O or document the real bound.

#### M-05: Unbounded thresholds can overflow freshness calculations

**Type:** Confirmed numeric edge-case defect  
**Evidence:** `src/config.rs:105-138`, `src/infrastructure/activitywatch/mapper.rs:207-213,252-264`

Configuration applies lower bounds but no maximums. `u64` values are added and cast to `i64`; sufficiently large AFK/stale settings can wrap, become negative, produce false staleness, or panic in debug callers. Add practical maximums and checked/saturating arithmetic with fallible conversions.

#### M-06: A 24-hour duration clamp falsifies valid long-running events

**Type:** Confirmed freshness defect  
**Evidence:** `src/infrastructure/activitywatch/mapper.rs:36-64`

An ActivityWatch event representing an unchanged state for more than 24 hours is truncated before its effective end is calculated. A current 48-hour AFK event can therefore appear a day stale. Replace the arbitrary clamp with checked timestamp arithmetic and explicit future-skew validation.

#### M-07: ActivityWatch identity verification is weaker than documented

**Type:** Security/trust risk  
**Evidence:** `src/infrastructure/activitywatch/client.rs:134-156`, `tests/mock_scenarios.rs:60-97`

Any successful JSON object containing nonempty `hostname` and `version` values passes the strict handshake. Existing wrong-service tests omit those common fields. Validate more of the API contract, but rely primarily on enforced loopback/authentication rather than treating this payload as cryptographic identity.

#### M-08: HTTP response bodies and normalized strings have no size limits

**Type:** Resource-exhaustion risk  
**Evidence:** `src/infrastructure/activitywatch/client.rs:130-145,192-196,235-242`

Server-controlled bodies are fully buffered, and `limit=1` is only a request hint. A wrong or malicious endpoint can return an enormous bucket map, event, or string. Enforce response byte, bucket count, and normalized string limits.

#### M-09: Request construction and error taxonomy are inconsistent

**Type:** Confirmed robustness/diagnostics defect  
**Evidence:** `src/infrastructure/activitywatch/client.rs:68-70,100-110,199-240`

Bucket IDs are interpolated into paths without percent-encoding; event requests do not map 401/403 to `AuthRequired`; several TLS, URL, DNS, redirect, body-timeout, and parse failures receive misleading categories; and the supposedly constructed client can reach an `expect`. Build paths with `Url`, centralize response/error classification, and make construction fallible.

#### M-10: Unknown provider values silently change backend behavior

**Type:** Confirmed configuration defect  
**Evidence:** `src/config.rs:30-42,93-140`, `src/infrastructure/mod.rs:20-36`

The provider is an unrestricted string and every unknown value falls through to automatic mode. Partial TOML files also do not inherit struct defaults as users might expect. Deserialize an enum, reject unknown fields/values, trim aliases deliberately, and apply `#[serde(default)]` intentionally.

#### M-11: Diagnostic privacy sanitization is incomplete

**Type:** Confirmed privacy defect  
**Evidence:** `src/infrastructure/native_windows/mod.rs:109-131`, `src/presentation/console.rs:62-72`, `src/infrastructure/activitywatch/mapper.rs:66-123`, `src/infrastructure/native_windows/api.rs:202-260`

Native doctor output includes the complete current title regardless of the unused verbose flag. URL sanitization retains sensitive paths; running-app enumeration returns raw unbounded titles; endpoint userinfo and server-controlled diagnostic strings can enter logs. Standard diagnostics should omit activity values and sanitize/cap all provider text, with an explicit redacted/verbose boundary.

#### M-12: Capabilities are inferred from metadata, not verified from usable data

**Type:** Health-model risk  
**Evidence:** `src/domain/capabilities.rs:3-19`, `src/infrastructure/activitywatch/discovery.rs:97-113`

Bucket names/types alone enable application/title/browser/incognito flags, including prefix-based incognito inference. Separate structural support, currently discovered channels, and fields actually observed as usable.

#### M-13: Session-to-workflow integrity and lifecycle invariants are not enforced in SQLite

**Type:** Confirmed schema gap  
**Evidence:** `src/infrastructure/persistence/sqlite/schema.rs:7-61`, `tests/sqlite_persistence_tests.rs:141-162`

`sessions.workflow_id` has no foreign key, and the schema accepts unknown enum strings, negative/impossible aggregates, invalid booleans, empty IDs, and terminal sessions without end times. One test explicitly saves a session for a workflow that was never inserted. Define history retention semantics, add validated references/snapshots, and enforce state/value checks.

#### M-14: Migration coordination can race and accepts incompatible version states

**Type:** Confirmed migration risk  
**Evidence:** `src/infrastructure/persistence/sqlite/schema.rs:68-96`

The maximum version is read before the migration transaction. Concurrent first opens can both observe version zero, while gaps, a database newer than the binary, and pre-existing incompatible `IF NOT EXISTS` tables are not rejected. Acquire a write reservation before inspecting versions, validate a contiguous known history and schema shape, reject downgrades, and back up before upgrades.

#### M-15: Workflow listing silently drops corrupt rows and mixes snapshots

**Type:** Confirmed repository defect  
**Evidence:** `src/infrastructure/persistence/sqlite/workflow_repository.rs:294-319`

`filter_map(|r| r.ok())` hides row conversion failures. IDs are then fetched separately after releasing the first connection guard, allowing concurrent changes to produce an incomplete mixed-time result. Propagate row errors and load a complete aggregate set within one read transaction.

#### M-16: In-memory and SQLite repositories implement different contracts

**Type:** Test-validity/design defect  
**Evidence:** `src/infrastructure/persistence/in_memory.rs:32-37,85-102`, `src/infrastructure/persistence/sqlite/schema.rs:19-30,54-61`, `src/infrastructure/persistence/sqlite/workflow_repository.rs:260-285`, `tests/workflow_service_tests.rs:210-216`

In memory, duplicate rule IDs across workflows and orphan drifts are accepted; SQLite rejects them. Child workflow IDs can be preserved, silently rewritten, or conflict depending on the adapter, and session immutable-field updates differ. A workflow service fixture relies on duplicate/mismatched child rules that production SQLite rejects. Define one port contract and run the same contract/lifecycle suite against both adapters.

#### M-17: Whole-aggregate saves permit lost updates

**Type:** Concurrency risk  
**Evidence:** `src/infrastructure/persistence/sqlite/connection.rs:10-12`, `src/infrastructure/persistence/sqlite/session_repository.rs:23-184`

The mutex serializes individual calls, not a `get`/modify/`save` sequence. Separate writers can read the same counters and overwrite each other's changes. Add optimistic revision checks, atomic SQL updates, or enforce a single writer.

#### M-18: Persistence formats and error types are not stable/actionable

**Type:** Maintainability/recovery defect  
**Evidence:** `src/infrastructure/persistence/sqlite/workflow_repository.rs:223,263`, `src/infrastructure/persistence/sqlite/session_repository.rs:143`, `src/application/ports.rs:95-106`

Enums are stored using Rust `Debug` names, making ordinary variant renames implicit data migrations. Busy, full, conflict, corrupt, I/O, and poisoned-lock failures all collapse into a string. Use explicit versioned database representations and preserve typed SQLite error categories for retry/recovery/UI handling.

#### M-19: Sensitive history defaults to a roaming or current-directory database

**Type:** Privacy/storage risk  
**Evidence:** `src/config.rs:221-233`, `src/infrastructure/persistence/sqlite/connection.rs:18-26`, `src/infrastructure/persistence/sqlite/schema.rs:40-42,57-60`

On Windows, `dirs::data_dir()` selects roaming `%APPDATA%`, not machine-local `%LOCALAPPDATA%`; fallback uses the current directory. Directory errors are ignored, permissions are not hardened, the full path is logged, and private reasons/activity targets remain plaintext in the database and WAL sidecars. Prefer `data_local_dir`, fail or warn loudly rather than silently using CWD, establish user-only ACLs, and implement documented retention/wipe/encryption choices.

#### M-20: The persistence boundary accepts unsanitized activity targets

**Type:** Privacy boundary gap  
**Evidence:** `src/domain/activity.rs:53-66`, `src/domain/workflow/entities.rs:94-102`, `src/infrastructure/persistence/sqlite/session_repository.rs:196-205`

`DriftEvent.target_name` accepts arbitrary text, so direct/future adapters can persist credentials, query strings, searches, fragments, or private paths even if the current mapper performs partial sanitization. Enforce canonical domain/resource storage at the domain or repository boundary.

#### M-21: GUI cadence ignores configuration and misaccounts short ticks/outages

**Type:** Confirmed timing defect  
**Evidence:** `src/presentation/gui/mod.rs:72-77`, `src/presentation/gui/engine.rs:56-57,84-85,141-146,244-264`

GUI polling is hard-coded around one second even though a configured interval is passed to the service. Start/resume forces an immediate evaluation, while accounting substitutes at least one full configured interval whenever elapsed whole seconds are zero. Source errors do not suspend the timer/accounting timestamp, so a later observation can charge outage time to a new state or expire grace. Schedule from monotonic instants and configured cadence, retain sub-second precision, and record unavailable time explicitly.

#### M-22: GUI API validation and workflow creation are internally inconsistent

**Type:** Confirmed validation defect  
**Evidence:** `src/presentation/gui/state.rs:95-117`, `src/presentation/gui/server.rs:138-253,354-444`

Names, durations, reflection requirements, rule counts, controls, apps, and domains are largely unbounded/unnormalized; slug collision checking and save are non-atomic. Start-session override fields are deserialized but ignored, malformed stop JSON becomes a valid no-reason request, and HTTP status mapping parses error strings. Add typed validation/ranges, atomic unique IDs, normalized IDNA/process values, typed errors, and either implement or remove unused contract fields.

#### M-23: The advertised YouTube resource lock creates nonfunctional title/domain rules

**Type:** Confirmed feature defect  
**Evidence:** `src/presentation/gui/static/index.html:662-675`, `src/presentation/gui/static/app.js:454-460,508-525`, `src/presentation/gui/server.rs:235-247`

The UI asks for a video/playlist URL but JavaScript rejects URLs and submits title keywords as `DomainPattern` rules. With a real browser URL, matching uses the hostname and never falls back to those keywords; no canonical video/playlist rule is created. Parse canonical IDs server-side, add exact-resource allow plus lower-specificity platform restriction, and test all supported URL forms.

#### M-24: Preset protection and Unicode workflow routes are inconsistent

**Type:** Confirmed GUI data-management defect  
**Evidence:** `src/presentation/gui/seed.rs:233-289`, `src/presentation/gui/server.rs:255-270`, `src/presentation/gui/static/app.js:259-267,673-683`

The seeded `wf-learning` preset is missing from protection lists and can be deleted permanently because seeding only runs for an entirely empty database. Unicode IDs are percent-encoded by the browser but not decoded by the server, making some workflows undeletable. Centralize preset metadata, repair missing presets by ID, use UUID/ASCII IDs, and decode/validate route parameters.

#### M-25: Frontend polling and modal state can regress to stale UI

**Type:** Confirmed client-state defect  
**Evidence:** `src/presentation/gui/static/app.js:692-717,747-754,810-870,943-958`, `src/presentation/gui/static/index.html:143`

Overlapping asynchronous interval polls allow old responses to overwrite new state. Inactive rendering leaves titles/modals stale; auto-completion can leave intervention open; the Unbreakable pause-disable code targets the wrong element ID; form state is incompletely reset. Serialize/cancel polling and render every surface from one explicit state transition with action controls disabled while pending.

#### M-26: GUI health presentation can be green while data is stale or unavailable

**Type:** Confirmed status defect  
**Evidence:** `src/presentation/gui/mod.rs:61-65`, `src/presentation/gui/state.rs:162-173,304-326`, `src/presentation/gui/engine.rs:217-259`

State starts connected before a successful poll; any `Ok` result sets connected without inspecting source health/freshness; warnings/unavailable decisions become policy `None`; and strict ActivityWatch can be labeled native fallback. Expose actual provider, health, freshness, last success, and warning/unavailable decisions.

#### M-27: Native alerts ignore mute and can beep every intervention tick

**Type:** Confirmed accessibility/usability defect  
**Evidence:** `src/presentation/gui/engine.rs:198-215`, `src/presentation/gui/static/app.js:14-116`

The frontend mute setting controls only Web Audio. Engine-native alerts continue, including a system beep on each later intervention tick, approximately every second. Centralize preferences, edge-trigger/rate-limit alerts, and respect mute and OS accessibility settings.

#### M-28: Live/history analytics use incompatible formulas and misleading totals

**Type:** Confirmed analytics defect  
**Evidence:** `src/presentation/gui/state.rs:350-355`, `src/presentation/gui/server.rs:276-293`, `src/presentation/gui/static/app.js:1284-1303`

Live scoring excludes grace while history includes it; a session score can drop after ending. Zero-duration sessions score 100%, and "total" analytics aggregate only the newest 50 rows including unfinished/interrupted/cancelled records. Define one formula, filter/label populations, and calculate true totals or name the window accurately.

#### M-29: Activity Radar uses synthetic seconds and classifies unknown activity as productive

**Type:** Confirmed analytics/classification defect  
**Evidence:** `src/presentation/gui/state.rs:177-245,439-633`

Each successful observation adds exactly one second regardless of real delay; the current slice is omitted until a switch and short slices are discarded. AFK can leave an old distraction banner visible. Unknown apps and ordinary YouTube default to productive, while broad substring matching is spoofable. Use monotonic intervals, snapshot the active slice, render AFK explicitly, and base alignment on policy or mark unknown neutrally.

#### M-30: Worker failure and server protocol/security handling are unsupervised

**Type:** Availability/security-hardening risk  
**Evidence:** `src/presentation/gui/mod.rs:128-135`, `src/presentation/gui/server.rs:37-51,97-120,448-507`, `src/presentation/gui/engine.rs:79-80`

Server-loop exit does not stop the main process, lock poisoning can kill workers, joins are ignored, unsupported methods return 404, and CSP/frame/nosniff/referrer/cache/cross-origin headers are absent. Supervise worker termination and panics, propagate fatal state, return correct methods, and add strict browser security headers after removing inline code.

#### M-31: Browser launch trusts user-writable candidates and discards ownership

**Type:** Platform security/lifecycle risk  
**Evidence:** `src/presentation/gui/mod.rs:140-184`

The launcher executes the first fixed-path browser found, including `%LOCALAPPDATA%`, without identity verification, uses the normal profile/extensions, and discards the child handle. Never elevate this process; prefer trusted OS registration or an isolated WebView profile, and retain verified process/window ownership.

#### M-32: Accessibility and narrow-window behavior are incomplete

**Type:** Missing quality requirement  
**Evidence:** `src/presentation/gui/static/index.html:26-39,380-566`, `src/presentation/gui/static/style.css:199-211,481-485,798-803,1343-1351`

Modals lack dialog semantics/focus management; dynamic status has no live region; clickable spans are not keyboard controls; focus outlines are removed; pressed/selected state and reduced-motion support are absent. The single breakpoint leaves fixed multi-column/header/timer layouts clipped on narrow screens, and several used utility classes are undefined. Add semantic controls/dialogs, visible focus, live regions, reduced-motion/contrast checks, missing styles, and 768/480/320px coverage.

#### M-33: Seeding and CLI runtime errors are suppressed or weakened

**Type:** Confirmed error-handling defect  
**Evidence:** `src/presentation/gui/seed.rs:10-16,72-73,127-128,184,231,289`, `src/main.rs:227-243,373-383`, `src/presentation/session_runner.rs:290-294`

Seed listing failure is treated as empty, seed writes are ignored, and partial presets are never repaired. Unknown CLI contract strings silently become Standard; zero settings are accepted; runner failures can print an error yet exit successfully and leave a session active. Propagate/repair seed failures, use typed bounded CLI values, return nonzero on failure, and persist interruption during unwinding.

#### M-34: Grace countdown displays zero while still in grace

**Type:** Confirmed boundary defect  
**Evidence:** `src/domain/workflow/timer.rs:99-108`

Remaining duration is floored to whole seconds, so a one-second timer commonly emits `GracePeriod { remaining_seconds: 0 }` and a new 30-second timer displays 29. Use ceiling semantics and test exact boundaries.

#### M-35: Detached timer resolutions can attach to a later episode

**Type:** Confirmed state-machine defect  
**Evidence:** `src/domain/workflow/timer.rs:67-97,119-143`, `src/application/workflow_service.rs:229-268`

Warning/unavailable handling can leave `last_resolution` unconsumed; a new violation does not clear it, and a later transition can persist that old result with the new target while the new timer is still active. Return a resolution as part of the exact state transition rather than storing a detached side channel.

#### M-36: Warn, Ignore, Monitor, and default effects do not implement their declared semantics

**Type:** Confirmed domain-semantics defect  
**Evidence:** `src/domain/workflow/decision.rs:7-47`, `src/domain/workflow/engine.rs:514-525,597-601`, `src/application/workflow_service.rs:288-305`

Warning is described as permitted but excluded from `is_allowed()` and receives no time accounting; default Warn becomes Ignored; Monitor is indistinguishable from Ignore; configured Ignore resolves grace while system Ignore pauses it. Separate permission, timer-resolution, intervention, and accounting dimensions and implement each effect consistently.

#### M-37: Exact URL and canonical resource normalization are incomplete

**Type:** Confirmed matching defect  
**Evidence:** `src/domain/workflow/entities.rs:71-73`, `src/domain/workflow/engine.rs:34-109,433-435`

ExactUrl is raw string equality despite its normalization contract, so fragments/query/order, host case, default ports, and trailing slash break equivalence. Canonical parsing misses valid platform variations and can classify unrecognized GitHub routes as repositories. Normalize configured and observed values through one platform-aware positive grammar.

#### M-38: Rule scope, matcher, and workflow ownership can contradict each other

**Type:** Domain-invariant risk  
**Evidence:** `src/domain/workflow/entities.rs:53-77,104-117`, `src/domain/workflow/engine.rs:488-502`

Callers can assign ExactResource scope to an `Any` matcher or attach another workflow's rule; provenance required by the documented workflow/global tie-break does not exist. Derive scope from matcher or validate all three attributes at construction and persistence boundaries.

#### M-39: System-surface ignoring is overbroad and incomplete

**Type:** Classification risk  
**Evidence:** `src/domain/workflow/engine.rs:354-368`

Every `applicationframehost.exe` window is ignored even when hosting a real UWP app, while other transient surfaces are absent. Use window class/lineage or explicit source metadata rather than executable name alone.

#### M-40: Hidden clocks undermine deterministic replay and safe contract timing

**Type:** Architecture/testability risk  
**Evidence:** `src/domain/workflow/engine.rs:313-332`, `src/domain/workflow/timer.rs:67-69`, `src/domain/workflow/session.rs:82-105,177-207`

The engine accepts wall time, but the timer calls `Instant::now()` and sessions call `Utc::now()` internally. Contract cancellation mixes aggregate counters with adjustable wall-clock duration, and workflow minutes can disagree with contract seconds. Inject monotonic/wall clocks and define one authoritative target/time basis with persisted checkpoints.

### Low

#### L-01: Stale observations can defeat meaningful-change suppression

**Type:** Confirmed output-noise defect  
**Evidence:** `src/domain/activity.rs:117-141`, `src/infrastructure/activitywatch/mapper.rs:332-337,392-397,449-452`

Diagnostics contain a continuously changing age while change detection compares diagnostics verbatim. An otherwise unchanged stale event can therefore trigger a callback every second. Compare stable typed diagnostic identities and render age separately.

#### L-02: Long native values and extreme idle intervals are truncated

**Type:** Edge-case risk  
**Evidence:** `src/infrastructure/native_windows/api.rs:83-140`

Fixed UTF-16 buffers can lose long process paths/titles, and 32-bit tick arithmetic cannot represent inactivity beyond roughly 49.7 days. Use dynamically sized retrieval where possible and document or handle the counter limitation.

#### L-03: SQLite concurrency, history indexing, and durability policy are incomplete

**Type:** Performance/durability risk  
**Evidence:** `src/infrastructure/persistence/sqlite/connection.rs:8-12,43-50`, `src/infrastructure/persistence/sqlite/schema.rs:51-52`, `src/infrastructure/persistence/sqlite/session_repository.rs:276-278`

All cloned repositories share one mutexed connection, the recent-session sort has no supporting index, effective WAL mode is not verified, and no vacuum/secure-deletion policy exists. Use a bounded worker/pool if concurrency is needed, add a total-order history index, verify pragmas, and define durability and deletion behavior.

#### L-04: Drift events have no stable idempotency identity

**Type:** Data-model risk  
**Evidence:** `src/domain/workflow/entities.rs:94-102`, `src/infrastructure/persistence/sqlite/schema.rs:54-61`

The only drift ID is SQLite autoincrement, so an uncertain retry can duplicate an event and exports cannot correlate it. Add a domain `DriftEventId`, unique insertion semantics, and deterministic chronological ordering.

#### L-05: Workflow mutation metadata and numeric operations are not guarded

**Type:** Maintainability/numeric risk  
**Evidence:** `src/domain/workflow/entities.rs:178-182`, `src/domain/workflow/session.rs:180-186`, `src/application/workflow_service.rs:242-303`

Rule mutation does not update `updated_at`, public fields bypass mutation metadata, and extreme/corrupt counters can overflow or narrow to `u32`. Encapsulate mutation, maintain timestamps repository-side, validate stored ranges, and use checked/saturating arithmetic deliberately.

## 5. Release And Supply-Chain Findings

### R-01: Application builds are not reproducible

**Severity:** High  
**Type:** Release risk  
**Evidence:** `.gitignore:3-4`, `Cargo.toml:1-30`, ignored `Cargo.lock`

`Cargo.lock` exists locally but is ignored and is not part of the repository. This is an application binary, for which Cargo convention is to commit the lockfile. A clean checkout can resolve different transitive versions than those used for either included executable. There is also no `rust-toolchain.toml`, release workflow, checksum manifest, SBOM, signed tag, or build attestation.

**Impact:** A release cannot be rebuilt byte-for-byte or reliably connected to a reviewed dependency graph. Dependency updates can enter a build without a source diff.

**Recommendation:** Commit `Cargo.lock`, pin the supported Rust toolchain, build releases in CI, publish checksums/SBOM/provenance, and tie each binary to a signed tag and source commit.

### R-02: Distributed executables have no publisher identity or source attestation

**Severity:** Medium  
**Type:** Release/security risk  
**Evidence:** `versions/v1/flowguard.exe`, ignored root `FlowGuard.exe`

Both artifacts are unsigned PE32+ x64 console executables and contain no file/product/company version resources. The tracked archive matches its Git blob, but its source commit is unsigned and no metadata proves which source and dependency set produced it. The ignored root executable has no Git integrity chain at all.

**Impact:** Users cannot verify publisher identity or tamper status; Windows SmartScreen warnings are likely. Static metadata can establish that the files look like Rust FlowGuard builds, not that their behavior matches the reviewed source.

**Recommendation:** Do not treat a checked-in executable as a release process. Produce signed, versioned artifacts in CI and publish hashes and provenance outside the source tree.

### R-03: The declared dual license is incomplete

**Severity:** High  
**Type:** Packaging/legal risk  
**Evidence:** `Cargo.toml:7`, `LICENSE:1-21`

The manifest declares `MIT OR Apache-2.0`, but only MIT license text is present; no Apache-2.0 license or notice exists. Manifest authorship and the MIT copyright holder also differ, and binary bundles contain no third-party notices.

**Recommendation:** Either declare MIT only or add complete Apache licensing/notice material, clarify ownership, and ship dependency notices with releases.

### R-04: Current test and quality claims are contradictory and unsubstantiated

**Severity:** High  
**Type:** Release-evidence defect  
**Evidence:** `docs/MILESTONE_1_ACCEPTANCE.md:23`, `docs/TESTING.md:101-104`, `docs/PROJECT_STATUS.md:89-91`, `code_reviews/code_review1.md:1196-1221`, `code_reviews/code_review2.md:286-307,364`

Documents claim 28, 57, 90, and 131 passing tests. Static inventory currently finds 139 test functions, which is not the same as an executed result. `tests/real_integration_test.rs:10-16` returns successfully on every handshake failure, so a green run need not contact ActivityWatch. No CI evidence is committed for tests, Clippy, warnings, flakes, or supported platforms.

**Recommendation:** Generate results in CI tied to commit, platform, toolchain, and command. Make live integration explicitly opt-in and fail strictly when enabled; remove hand-maintained totals and unconditional pass claims.

### R-05: Published performance figures are generated by an invalid benchmark

**Severity:** High  
**Type:** Evidence defect  
**Evidence:** `scripts/benchmark.ps1:25-30`, `docs/PERFORMANCE.md:31-42`, `code_reviews/code_review1.md:1082-1085`

The script always prints `(< 0.05% CPU)` without calculating CPU utilization, samples only a few seconds, has no repetitions/variance/environment/raw results/readiness/failure handling, and force-terminates the process. It measures the old observe CLI rather than the GUI, SQLite, native source, API polling, and interventions. The reported 3.2 MB binary also conflicts with the 7.56 MB tracked executable.

**Recommendation:** Remove current release claims. Benchmark each supported mode with real wall/CPU normalization, repetitions and soak tests, raw versioned results, machine/toolchain/config metadata, commit and binary hash.

### R-06: Existing `code_review2.md` release certification is materially inaccurate

**Severity:** High  
**Type:** Governance/documentation defect  
**Evidence:** `code_reviews/code_review2.md:64-132,334-364`

The historical review describes a tracked lockfile, custom release profile, migrations directory, different source paths, root executable availability, exponential backoff, working-directory database, `--gui` syntax, and M1–M7 completion that the current repository does not support. It missed the unauthenticated API and is not bound to a reviewed commit.

**Recommendation:** Mark it superseded and withdraw release certification. Future sign-off must name an immutable commit and include test, build, security, provenance, and unresolved-finding evidence.

## 6. Missing Tests And Validation

Static inspection finds 139 Rust test functions (138 integration tests and one timer unit test), but no test/build command was run during this review and repository documentation contains no current CI result. The highest-value missing validation is:

| Area | Required validation |
|---|---|
| Local API security | Reject missing/invalid capabilities, foreign Origin/Host, DNS-rebinding Host values, simple CSRF POSTs, wildcard CORS, oversized/slow bodies, excessive concurrency, clickjacking, missing security headers, and hostile XSS payloads including HTML entities. |
| Commitment and timers | Reject early completion for every contract; test zero grace, exact expiry, ceiling display, return after deadline without an expiry poll, changed grace across rule hops, repeated pause/resume, stale browser/idle data, source-error gaps, title spoofing, disabled/expired domain rules, and explicit short-form exceptions. |
| Temporal accounting | Use an injectable clock to test subsecond/non-default polling, delayed ticks, transition attribution, AFK/pause/outage intervals, CLI/GUI target equivalence, wall-clock changes, suspend/resume, and completion during grace/intervention. |
| Persistence failure injection | Exercise busy, disk-full, corrupt, constraint, serialization, and poisoned-lock failures at every transition. Verify rollback/retry, atomic session-plus-drift commits, paused persistence, terminal violations, and restart recovery. |
| Repository contract | Run one suite against SQLite and in-memory for duplicate IDs, child ownership, orphans, immutable updates, order, error types, complete round trips, corrupt rows, list limits, and deterministic ties. Use file-backed reopen tests, not only in-memory SQLite. |
| Database concurrency/migrations | Test two connections/processes starting simultaneously, same-millisecond IDs, lost updates, live-owner recovery, concurrent first migration, gaps/newer versions, incompatible existing tables, rollback, backup, WAL/pragma verification, and query plans. |
| Activity sources | Cover loopback URL enforcement, userinfo/redirect/proxy/secret selection, unknown providers, partial defaults, foreign-host-only buckets, watcher appearance/removal, rediscovery retry, capability recomputation, automatic failback, fresh malformed payloads, huge bodies, long events, numeric limits, shutdown latency, and non-Windows behavior. |
| GUI lifecycle/frontend | Verify browser-close and Ctrl+C shutdown, Active/Paused finalization, worker panic supervision, expired command cancellation, concurrent workflow creation, UI self-classification, empty-selection semantics, serialized polling, modal cleanup, button IDs, Unicode routes, muted alerts, and 320/480/768px accessibility behavior. |
| Privacy/output | Fuzz ANSI/OSC/C0/C1/newline/bidi titles, URL credentials/query/fragment/private paths, stored drift targets, reflection text, title limits, browser caching, local database path/ACL, retention, wipe, and WAL sidecars. |
| Release evidence | Build from a clean checkout with a committed lock/toolchain, run tests/Clippy/audit/license checks in CI, produce checksums/SBOM/attestation, verify PE metadata/signing, and benchmark all supported modes with versioned raw data. |

Tests that currently give misleading confidence include:

- `tests/real_integration_test.rs:10-16` returns success for any ActivityWatch handshake failure.
- `tests/workflow_engine_tests.rs:445-463` and `tests/workflow_service_tests.rs:261-276` accept immediate Unbreakable completion.
- Stale-data tests start with an idle timer and therefore do not verify freezing an existing violation.
- Anti-reset timer assertions allow a full reset to pass because they only check `remaining_seconds <= 30`.
- Workflow service tests encode one synthetic second per immediate function call rather than elapsed time.
- SQLite tests use `open_in_memory()`, leaving production paths, WAL, reopen durability, busy behavior, and permissions untested.
- The mock reconnection test creates a new adapter for each phase and does not prove recovery of one instance.
- GUI tests mostly assert that static files contain strings; they do not execute browser behavior, security boundaries, timing, shutdown, or accessibility.

## 7. Product And Feature Gaps

The repository implements meaningful work beyond M1, but it does not implement the full master proposal. Current visible status is:

| Milestone/capability | Static-review status |
|---|---|
| M1 ActivityWatch integration | Implemented with material configuration, discovery, health, retry, privacy, and identity defects listed above. |
| M2 workflow engine | Substantially implemented, but contract completion, timers, time accounting, rule effects/precedence, and drift metrics are not reliable enough for strict enforcement. |
| M3 desktop and persistence | SQLite and an embedded browser GUI exist. The proposed Tauri/React/tray architecture and separate migration assets do not. Persistence is not transactionally integrated with session state. |
| M4 intervention system | One browser modal/audio/native-alert path exists. The documented four-tier notification engine, robust escalation/rate limiting, safe ownership, and emergency override do not. |
| M5 browser intelligence | Backend URL parsers and title heuristics exist. No Manifest V3 extension, authenticated browser IPC, or dependable exact video/playlist flow exists. Native title inference is advisory and spoofable. |
| M6 behavioral features | Research parking lot, distraction journal, learning checkpoints, smart breaks, return-to-work assistant, retention controls, export, and one-click wipe are absent. |
| M7 native monitoring | Polling through Win32 foreground/idle APIs exists. The proposed event-hook/message-pump/UI Automation/UWP resolver architecture does not. |

The most consequential product-contract gaps are:

- There is no unconditional emergency override despite repeated safety guarantees; process termination remains the real escape hatch.
- "Unbreakable" is neither safely unbreakable nor safely breakable: public completion bypasses it, while no in-app emergency override exists.
- Exact-resource YouTube locking is not delivered by the GUI workflow; title heuristics must not be marketed as strict browser enforcement.
- The meaning of target focus duration differs across GUI, CLI, pause, AFK, grace, and distraction paths.
- Closing the desktop-looking browser window does not terminate monitoring or seal the active session.
- The product has no implemented data retention, purge, encryption, backup/restore, or privacy dashboard despite storing reflections and drift targets.
- There is no supported multi-instance ownership model even though dynamic-port fallback permits multiple GUIs on one database.
- Activity-source degradation and native fallback can silently remove URL-level precision without a reliable user-visible warning or automatic recovery.

## 8. Documentation Accuracy

Documentation currently mixes three incompatible states: the historical M1 ActivityWatch foundation, a proposed Tauri/MV3 M2-M7 system, and the current Win32/SQLite/browser-GUI implementation. Proposal documents can remain useful, but they must be clearly separated from current behavior and release guarantees.

| Artifact | Accuracy assessment |
|---|---|
| `README.md` | Describes M1/ActivityWatch CLI rather than the default GUI, native fallback, SQLite, workflow/session/history commands, local API, database location, or release security. The Rust 1.80+ claim is not CI-backed. |
| `CHANGELOG.md` | Stops at old M1 work; current workflow, persistence, native, GUI, and intervention changes have no current Unreleased/release record. |
| `Cargo.toml` / `.gitignore` | Missing `rust-version`, release provenance/publish controls, and a tracked application lockfile; root executables are hidden while the archive binary is re-included. |
| `config/flowguard.example.toml` | Omits provider enum values, strict/auto behavior, permanent fallback, remote-host/token risk, GUI port, database path, and unknown-field behavior. |
| `docs/ACTIVITYWATCH_INTEGRATION.md` | GET-only/latest-event behavior is broadly supported, but auth, AutoSource, strict mode, versions, token threat model, and degraded native fidelity are missing. |
| `docs/ARCHITECTURE.md` | Tauri/native state is stale and SQLite, browser HTTP GUI, workflow engine, security boundary, and current `diagnose` port are absent. |
| `docs/CODE_MAP.md` | Inventories only old M1 code/tests and uses machine-local `file:///d:/...` links. It is no longer a current navigation map. |
| `docs/MASTER_PRODUCT_AND_ARCHITECTURE_SPEC.md` | Correctly says proposal near the top, but later mixes desired guarantees with implementation checklists. Tauri/React, extension, tray, hooks, interventions, behavior features, retention, backups, emergency override, and much of the proposed schema are absent. |
| `docs/MILESTONE_1_ACCEPTANCE.md` | Historical evidence lacks a commit/toolchain/log bundle; its "documentation matches code" condition is false for the present revision. |
| `docs/PERFORMANCE.md` | Reports obsolete, non-reproducible figures from a script that does not calculate CPU correctly and does not exercise the current application modes. |
| `docs/PRIVACY.md` | M1-only latest-event/memory model omits SQLite/WAL, titles, drift targets, exit reasons, local API exposure, retention, deletion, permissions, and remote configuration. |
| `docs/PRODUCT_VISION.md` | Aspirationally useful, but zero-overhead, exact-resource, and absolute local/privacy statements need measurable qualifications and links to implementation status. |
| `docs/PROJECT_STATUS.md` | Historical M1.1 snapshot presented as current; test and named-machine results are not retained as reproducible evidence. |
| `docs/ROADMAP.md` | Still labels M1 current, while native polling, workflow, SQLite, and GUI arrived in a different order/architecture. |
| `docs/TESTING.md` | Obsolete 90-test inventory and missing current security, local API, persistence-failure, packaging, accessibility, and release plans. |
| `docs/included_features.md` | Overstates SetWinEventHook, strict locking/resource detection, daily metrics, and modal accessibility. |
| `docs/future_features.md` | Ideas lack acceptance/privacy dependencies; its proposed port conflicts with current GUI port and another port in the master proposal. |
| `docs/decisions/ADR-001-*` | Accepted ActivityWatch-only decision is superseded by native fallback and should be marked accordingly. |
| `docs/decisions/ADR-002-*` | Still Proposed despite implementation; its trait example is stale and direct GUI-native calls weaken the claimed exclusive port boundary. |
| `docs/decisions/ADR-003-*` | M1 privacy decision omits current persisted/API data, retention, and threat model; "zero risk" wording is not defensible. |
| `code_reviews/code_review1.md` | Useful historical walkthrough but unanchored to a commit and obsolete in inventory, test counts, line numbers, architecture, and safety conclusions. |
| `code_reviews/code_review2.md` | Materially inaccurate current/release certification; see R-06. |

Narrower claims supported by visible source are: ActivityWatch traffic is GET-only; default ActivityWatch/GUI endpoints are loopback; the `ActivitySource` abstraction and native fallback exist; SQLite enables foreign keys, WAL, a busy timeout, and `synchronous=NORMAL`; and no screenshot capture, keylogging, telemetry SDK, or cloud AI client is visible. These source observations do not authenticate either executable or justify absolute guarantees under arbitrary configuration.

## 9. Recommended Remediation Order

1. Disable external GUI access until C-01 is fixed: add request capabilities, exact Origin/Host checks, restrictive CORS/CSRF controls, body/concurrency limits, no-store/security headers, and remove DOM-XSS sinks.
2. Fix C-02 and define the safety contract: one authoritative monotonic duration, domain-enforced completion, a real emergency override, and honest naming for commitment modes.
3. Replace the split timer/service flags with one episode state machine that freezes uncertainty, handles rule/deadline changes, seals events, and drives intervention counting and time accounting.
4. Make lifecycle persistence transactional and retryable: preserve in-memory state on failure, atomically commit session plus drift, surface storage degradation, generate unique IDs, and enforce one live owner/session.
5. Repair privacy boundaries: loopback-only default configuration, constrained secret handling, canonical/redacted targets, local user-only database storage, current retention/wipe documentation, and terminal-safe output.
6. Correct source health and recovery: same-host buckets, periodic rediscovery, automatic failback, real native probes, fresh doctor handshakes, schema validation, bounded responses, and consistent backoff/error types.
7. Fix user-visible workflow behavior: neutral FlowGuard control surface, explicit empty-selection semantics, canonical YouTube resource rules, correct CLI preset IDs, shutdown/finalization, queued-command cancellation, and accurate health/provider display.
8. Establish the failure/security test suites in Section 6 before adding features. Run the same repository contract against SQLite and memory, and add browser-level tests rather than static string assertions.
9. Restore release integrity: commit `Cargo.lock`, pin/test a Rust version, resolve licensing, add CI audit/build/test checks, produce signed versioned artifacts with checksums/SBOM/provenance, and remove opaque binaries from source-release claims.
10. Publish one authoritative implementation matrix, current privacy/security model, installation/operation/API guide, and database backup/delete guidance; mark historical reviews/status/ADRs as archived or superseded.
11. Rebuild analytics and performance evidence from real monotonic intervals and current modes; remove unsupported CPU, memory, leak, binary-size, zero-flake, and release-readiness claims until reproducible evidence exists.
12. Complete accessibility and responsive work with semantic dialogs/controls, focus management, live regions, reduced-motion support, keyboard tests, contrast review, and narrow-window visual tests.

## 10. Review Limitations

- Static review cannot prove runtime behavior, performance, binary/source equivalence, Windows API behavior, or database crash recovery.
- Dependency versions were inspected from the local ignored lockfile, but no vulnerability database scan was run.
- The checked-in and ignored executables were not executed.
- Statements such as "all possible bugs" cannot be guaranteed by any finite review. This document records the concrete defects and credible edge cases found in the reviewed revision.
