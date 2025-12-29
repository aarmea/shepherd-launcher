This is the initial prompt to create `shepherdd` and its supporting crates, provided to Claude Opus 4.5 via VSCode Agent mode with nothing other than a Rust "hello world".

It was produced by ChatGPT after some back-and-forth [in this conversation](https://chatgpt.com/share/6952b1fb-35e4-800b-abca-5f212f9d77e5).

The output was committed as ac2d2abfed39e015aed6c2400f4f4609a2823d6d.

-----

Implement `shepherdd` as described below.

### Prelude: What is `shepherdd`, and why does it exist?

`shepherdd` is the **authoritative policy and enforcement daemon** for a child-focused, parent-defined computing environment. Its job is not to be a launcher UI, a window manager, or a media player. Its job is to **decide what is allowed, when it is allowed, for how long it is allowed, and to enforce that decision reliably**—regardless of what user interface, operating system, or legacy application happens to be in use.

The problem it solves is simple to describe but hard to implement correctly:

> Parents want to give children access to *real software*—including legacy games, emulators, virtual machines, browsers, and media—without relying on opaque vendor-controlled ecosystems or fragile UI-level restrictions. Time limits must be enforced. Warnings must be accurate. Expiry must be non-negotiable. And the system must remain understandable, inspectable, and extensible over time.

`shepherdd` is the **root of trust** for this environment. Everything else—launchers, overlays, shells, admin apps—is replaceable.

---

### Core philosophy

1. **Policy, not UI, is the authority**
   `shepherdd` decides *what happens*. User interfaces only request actions and display state. If a UI crashes, disconnects, or is replaced entirely, enforcement continues.

2. **Real software, unmodified**
   The system must be able to supervise arbitrary third-party programs:
   emulators (e.g. ScummVM), virtual machines (e.g. Windows 9x games), browsers, media players, and other legacy or closed software that cannot be instrumented or embedded. Enforcement cannot rely on cooperation from the application.

3. **Time is enforced, not suggested**
   Sessions have fixed deadlines computed at launch. Warnings occur at defined thresholds. When time expires, the session is terminated—gracefully if possible, forcefully if necessary.

4. **Portability over cleverness**
   Desktop operating systems have fundamentally different control models. Linux can kill process groups; Android cannot. macOS requires MDM for true kiosk mode; Windows uses job objects and shell policies. `shepherdd` must acknowledge these differences honestly through a capability-based host interface rather than pretending all platforms are equivalent.

5. **Low-level, but not reckless**
   `shepherdd` is designed to run as a background service with elevated privileges where appropriate, but it avoids OS-specific assumptions in its core. Platform-specific behavior lives behind explicit adapters.

6. **Open, inspectable, and extensible**
   Policies are human-readable. Decisions are explainable (“why is this unavailable?”). Actions are logged. The architecture is intended to invite future contributors—especially for additional platforms—without requiring them to understand the entire system.

---

### What `shepherdd` is *not*

* It is **not** a graphical launcher.
* It is **not** a window manager or compositor.
* It is **not** a parental surveillance tool.
* It does **not** depend on cloud services.
* It does **not** assume a particular desktop environment or vendor ecosystem.

---

### How the system is expected to be used

* On **Linux/Wayland**, `shepherdd` runs as a service.
  It starts or coordinates with a compositor (e.g. Sway), launches supervised applications, and communicates with a launcher UI and an always-on HUD overlay via local IPC.

* On **Windows**, a future adapter may integrate with a custom shell or kiosk configuration while preserving the same policy and enforcement logic.

* On **macOS**, `shepherdd` may operate in “soft kiosk” mode or integrate with MDM / Autonomous Single App Mode for hard lockdowns, using the same core.

* On **Android**, the same policy engine could back a managed launcher and device-owner workflow, even though enforcement primitives differ.

In all cases, **the daemon is the entry point**: shells connect to it; admin tools manage it; enforcement flows from it.

---

### What you, the coding agent, are building

You are not building a UI.
You are building a **policy engine and enforcement service** that:

* loads and validates a parent-defined policy,
* evaluates availability and time limits,
* tracks and supervises running sessions,
* emits warnings and state changes,
* terminates sessions when required,
* exposes a stable IPC API for multiple frontends,
* and does so in a way that can survive OS differences and future expansion.

If this daemon is correct, the rest of the system can evolve freely. If it is wrong, no amount of UI polish will fix it.

That is why `shepherdd` exists.

-----

# `shepherdd` library requirements document

This document specifies the **libraries (crates/modules)** that a coding agent must implement to build `shepherdd` from scratch. It assumes **zero prior context** and defines the architecture, responsibilities, APIs, and non-functional requirements needed to support a portable “policy + enforcement” daemon with replaceable frontends and host adapters.

The implementation language is assumed to be **Rust** (recommended for portability + service-style reliability), but the requirements are written so the design could be ported.

---

## 0. Glossary

* **Daemon (`shepherdd`)**: The authoritative service that loads policy, decides what’s allowed, tracks time, issues warnings, and enforces expiry.
* **Shell / Frontend**: UI applications (e.g., Wayland launcher grid, HUD overlay, Windows shell, macOS kiosk UI) that display state and send commands. Shells do not enforce policy.
* **Host Adapter**: Platform-specific integration that can spawn/stop apps and (optionally) manage focus/fullscreen/lockdown.
* **Entry**: A whitelisted launchable unit (command, VM recipe, media collection).
* **Session**: A running instance of an Entry with start time, deadline, warnings, and enforcement actions.

---

## 1. Top-level library set

Implement the following libraries (Rust crates/modules), each testable in isolation:

1. **`shepherd-core`**
   Pure, platform-agnostic policy engine and session state machine.

2. **`shepherd-host-api`**
   Capability-based trait interfaces for platform adapters (Linux/Windows/macOS/Android). No platform code.

3. **`shepherd-host-linux`** (initial implementation target)
   Linux host adapter: spawn/kill process trees, optional cgroups, optional Sway IPC integration hooks.

4. **`shepherd-config`**
   Config schema, parsing, validation, and hot-reload support.

5. **`shepherd-store`**
   Persistence abstraction + at least one concrete backend for:

   * audit log
   * usage/accounting
   * current state recovery

6. **`shepherd-ipc`**
   Local privileged IPC transport and protocol implementation (Unix domain socket for Linux). Authentication/authorization for local clients.

7. **`shepherd-api`**
   Protocol types (request/response/event structures) shared by daemon and clients. Must be stable and versioned.

8. **`shepherd-remote`** (optional but designed-in; may be stubbed initially)
   Remote management plane over HTTPS with pairing/auth. Must not be required for local operation.

9. **`shepherd-util`**
   Shared utilities (time, IDs, error types, rate limiting helpers, etc.).

The `shepherdd` binary will mostly wire these together. This document focuses on the libraries.

---

## 2. System-level goals (must be supported by libraries)

### 2.1 Core capabilities

* Maintain a **whitelist** of allowed entries.
* Restrict entries by:

  * **time windows** (day-of-week + time-of-day windows)
  * **max run duration per launch**
  * optional **daily quotas** and **cooldowns**
* Provide system status needed by shells:

  * current session, time remaining, upcoming warnings
  * “why is this entry unavailable?”
* Emit warnings at configured thresholds.
* Enforce expiry by terminating the running session via host adapter.
* Support “media playback entries” using the same policy primitives.

### 2.2 Portability and extensibility

* Core logic must not assume Wayland, process-killing, or fullscreen exist.
* Enforcement must be expressed in terms of **capabilities**.
* Protocol must support multiple shells and admin tools simultaneously.
* The daemon must run as a service; shells connect over local IPC.

### 2.3 Safety and determinism

* The daemon is the **sole authority** for timing and enforcement.
* Must be resilient to UI crashes/disconnects.
* Timing must use **monotonic time** for countdowns (wall-clock changes must not break enforcement).
* All commands must be auditable.

---

## 3. `shepherd-core` requirements

### 3.1 Responsibilities

* Parse validated config objects (from `shepherd-config`) into internal policy structures.
* Evaluate policy at a given time (“what entries are visible/launchable now?”).
* Implement session state machine:

  * `Idle`
  * `Launching`
  * `Running`
  * `Warned` (can be multiple warning levels)
  * `Expiring` (termination initiated)
  * `Ended` (with reason)
* Compute:

  * allowed duration if started “now” (min of entry max duration, time-window end, quota remaining)
  * warning schedule times
* Produce a deterministic **Enforcement Plan**:

  * what to do on launch
  * when to warn
  * when/how to expire

### 3.2 Data structures

#### 3.2.1 Entry model

Minimum fields:

* `entry_id: EntryId` (stable)
* `label: String`
* `icon_ref: Option<String>` (opaque reference; shell interprets)
* `kind: EntryKind`:

  * `Process { argv: Vec<String>, env: Map, cwd: Option<Path> }`
  * `Vm { driver: String, args: Map }` (generic “VM recipe”)
  * `Media { library_id: String, args: Map }`
  * `Custom { type_name: String, payload: Value }`
* `availability: AvailabilityPolicy`
* `limits: LimitsPolicy`
* `warnings: WarningPolicy`

#### 3.2.2 AvailabilityPolicy

* `time_windows`: list of windows:

  * days-of-week mask
  * start time-of-day (local time)
  * end time-of-day (local time)
* semantics: entry is allowed if “now” is in at least one window.

#### 3.2.3 LimitsPolicy

* `max_run: Duration`
* optional:

  * `daily_quota: Duration`
  * `cooldown_after_run: Duration`
  * `min_gap_between_runs: Duration` (alias of cooldown)
* Must support future extension.

#### 3.2.4 WarningPolicy

* list of thresholds in seconds before expiry (e.g., 300, 60, 10)
* each threshold has:

  * `message_template` (optional)
  * `severity` (info/warn/critical)
* warnings must not be emitted twice for the same session.

### 3.3 Public API

* `CoreEngine::new(policy: Policy, store: StoreHandle) -> CoreEngine`
* `CoreEngine::list_entries(now) -> Vec<EntryView>`

  * `EntryView` includes:

    * enabled/disabled
    * reasons list (structured codes)
    * if enabled: `max_run_if_started_now`
* `CoreEngine::request_launch(entry_id, now) -> LaunchDecision`

  * Returns either `Denied { reasons }` or `Approved { session_plan }`
* `CoreEngine::start_session(approved_plan, host_session_handle, now)`
* `CoreEngine::tick(now_monotonic) -> Vec<CoreEvent>`

  * Emits scheduled warnings and expiry events.
* `CoreEngine::notify_session_exited(exit_info, now) -> CoreEvent`
* `CoreEngine::stop_current(request, now) -> StopDecision`

  * request may be user stop vs admin stop vs policy stop.
* `CoreEngine::get_state() -> DaemonStateSnapshot`

### 3.4 Event model

Core emits events for IPC and host enforcement:

* `SessionStarted`
* `Warning { threshold, remaining }`
* `ExpireDue`
* `SessionEnded { reason }`
* `EntryAvailabilityChanged` (optional; used for UI updates)
* `PolicyReloaded` (optional)

### 3.5 Time rules

* All “countdown” logic uses monotonic time.
* Availability windows use wall-clock local time.
* If wall-clock jumps, session expiry timing remains based on monotonic time, but **no extension** should be granted by clock rollback.

### 3.6 Testing requirements

* Unit tests for:

  * time window evaluation (DST boundaries included)
  * quota accounting
  * warning schedule correctness
  * state machine transitions
* Property tests recommended for “no double warning / no negative remaining / no orphan state”.

---

## 4. `shepherd-host-api` requirements

### 4.1 Responsibilities

Define the interface between core daemon logic and platform integration.

### 4.2 Capability model

`HostCapabilities` must include (at minimum):

* `spawn_kind_supported: Set<EntryKindTag>` (Process/Vm/Media/Custom)
* `can_kill_forcefully: bool`
* `can_graceful_stop: bool`
* `can_group_process_tree: bool` (process group / job object)
* `can_observe_exit: bool`
* `can_observe_window_ready: bool` (optional)
* `can_force_foreground: bool` (optional)
* `can_force_fullscreen: bool` (optional)
* `can_lock_to_single_app: bool` (MDM/Assigned Access style, optional)

### 4.3 SessionHandle abstraction

`HostSessionHandle` must be opaque to core:

* contains platform-specific identifiers (pid/pgid/job object, bundle ID, etc.)
* must be serializable if you plan crash recovery; otherwise explicitly “not recoverable”.

### 4.4 Host API trait

Minimum methods:

* `capabilities() -> HostCapabilities`
* `spawn(entry: Entry, spawn_opts: SpawnOptions) -> Result<HostSessionHandle>`
* `stop(handle, mode: StopMode) -> Result<()>`

  * `StopMode`: `Graceful(Duration)` then `Force`
* `subscribe() -> HostEventStream` (async)

  * `HostEvent`: `Exited(handle, exit_status)`, optionally `WindowReady(handle)`, `SpawnFailed`, etc.
* Optional (feature-gated):

  * `set_foreground(handle)`
  * `set_fullscreen(handle)`
  * `ensure_shell_visible()` (return to launcher)
  * `start_shell_process()` / `start_compositor()` (Linux-specific convenience, but keep generic naming)

---

## 5. `shepherd-host-linux` requirements (initial adapter)

### 5.1 Spawn/kill semantics

* Must spawn `Process` entries without invoking a shell:

  * `execve(argv[0], argv, env)` behavior
* Must place spawned process in its own **process group**.
* Must be able to terminate entire process group:

  * `SIGTERM` then `SIGKILL` after grace timeout.
* Must handle “VM recipes” as process spawns (e.g., launching qemu) using same group semantics.

### 5.2 Optional containment (extensible)

Design hooks for future:

* cgroups v2 creation per session (cpu/mem/io limits optional)
* namespacing (optional)
* environment sanitization and controlled PATH

### 5.3 Observability

* Must provide exit notifications for sessions.
* Should capture stdout/stderr to logs (file per session or journald).

### 5.4 Linux compositor management (optional module)

Provide an optional module that:

* can start Sway (or connect to existing)
* can issue minimal compositor commands (focus/fullscreen)
* can subscribe to compositor events (window created/destroyed)
  This module must be optional so the daemon can run headless or under another compositor.

---

## 6. `shepherd-config` requirements

### 6.1 Config format

* Must support TOML (recommended). YAML acceptable but TOML preferred for strictness.
* Provide versioned schema with explicit `config_version`.

### 6.2 Validation

Must reject invalid config with clear errors:

* duplicate entry IDs
* empty argv
* invalid time windows
* warning thresholds >= max_run
* negative durations
* unknown kinds/drivers (unless `Custom` allows unknown)

### 6.3 Hot reload

* Must support reload on SIGHUP (Linux) or an API call.
* Reload must be atomic:

  * either new policy fully applied, or old remains.
* On reload, current session continues with old plan unless explicitly configured otherwise.

---

## 7. `shepherd-store` requirements

### 7.1 Responsibilities

Persist:

* audit events (append-only)
* usage accounting (per entry/day)
* cooldown tracking
* last-known daemon snapshot (optional)

### 7.2 Backends

Minimum viable:

* SQLite backend (recommended)
  OR
* append-only JSON lines log + periodic compacted summary

### 7.3 API

* `Store::append_audit(event)`
* `Store::get_usage(entry_id, day) -> Duration`
* `Store::add_usage(entry_id, day, duration)`
* `Store::get_cooldown_until(entry_id) -> Option<Time>`
* `Store::set_cooldown_until(entry_id, time)`
* `Store::load_snapshot() -> Option<Snapshot>`
* `Store::save_snapshot(snapshot)`

### 7.4 Crash tolerance

* audit log must not corrupt easily
* writes must be durable enough for “time accounting correctness” (use transactions if SQLite)

---

## 8. `shepherd-api` requirements (protocol types)

### 8.1 Versioning

* `api_version: u32`
* Backward compatibility policy:

  * minor additions are allowed
  * breaking changes require version bump

### 8.2 Core types

* `EntryId`, `SessionId`, `ClientId`
* `EntryView` (for UI listing)
* `DaemonStateSnapshot`
* `ReasonCode` enum (structured unavailability reasons)
* `Command` enum + request/response wrappers
* `Event` enum for streaming

### 8.3 Commands (minimum)

Local privileged plane must support:

* `GetState`
* `ListEntries { at_time? }`
* `Launch { entry_id }`
* `StopCurrent { mode }`
* `ReloadConfig`
* `SubscribeEvents`

Optional admin:

* `ExtendCurrent { by }` (must be capability/role gated)
* `SetPolicy { policy_blob }` (remote plane only or locked down)

Events (minimum):

* `StateChanged(snapshot)`
* `SessionStarted`
* `WarningIssued`
* `SessionExpired`
* `SessionEnded`
* `PolicyReloaded`
* `AuditAppended` (optional)

---

## 9. `shepherd-ipc` requirements (local plane)

### 9.1 Transport

Linux MVP:

* Unix domain socket at a configurable path.
* newline-delimited JSON (NDJSON) **or** length-prefixed binary frames (protobuf). Either is acceptable, but must be explicitly framed.

### 9.2 Multiplexing

Must support:

* multiple concurrent clients
* at least one client subscribed to events
* request/response on the same connection (duplex) or separate connections; either is fine.

### 9.3 AuthN/AuthZ (local)

Minimum:

* rely on filesystem permissions of socket path
* identify peer UID (where supported) and attach to `ClientInfo`
  Roles:
* `shell` (UI/HUD)
* `admin` (local management tool)
* `observer` (read-only)

If peer UID is unavailable on future platforms, design the interface so other mechanisms can be plugged in.

### 9.4 Rate limiting

* per-client command rate limits to avoid UI bugs DOSing the daemon.

---

## 10. `shepherd-remote` requirements (remote plane; optional but designed)

### 10.1 Transport

* HTTPS server (TLS required)
* REST and/or gRPC acceptable
* Must be optional/off by default.

### 10.2 Pairing

Must support a pairing flow that does not require cloud:

* device generates a one-time code displayed on the local shell
* remote client uses code to obtain a long-lived credential
* credential stored securely on device

### 10.3 Authorization

Remote actions must be scoped:

* `read-only`
* `policy edit`
* `approve/deny/extend`

### 10.4 Security basics

* TLS
* brute-force protection on pairing
* audit log of remote actions

---

## 11. Cross-cutting requirements

### 11.1 Logging

* Structured logs
* Per-session logs linkable to `session_id`
* Separate audit log (non-repudiation style) from debug logs

### 11.2 Metrics/health

Expose health endpoints via local IPC:

* liveness (“daemon loop running”)
* readiness (“policy loaded”, “host adapter OK”)

### 11.3 Configuration of privilege

Libraries must not assume root/admin.

* Host adapter exposes what it can do.
* Daemon must degrade gracefully if running unprivileged (e.g., no cgroups).

### 11.4 Deterministic enforcement

* If an entry is launched, daemon must compute a fixed deadline and enforce it regardless of UI state.
* Warn/expire must be emitted at-most-once.

---

## 12. Deliverables checklist for a coding agent

A coding agent implementing these libraries must produce:

1. `shepherd-core` with full unit tests for time windows, warnings, quotas.
2. `shepherd-host-api` trait + capabilities, with mock host for tests.
3. `shepherd-host-linux` minimal:

   * spawn process group
   * graceful+force kill
   * exit observation
4. `shepherd-config` parse+validate TOML.
5. `shepherd-store` (SQLite or append-only log) with basic usage accounting.
6. `shepherd-api` command/event types + versioning.
7. `shepherd-ipc` local UDS server:

   * handle commands
   * event subscription
   * peer UID auth (where available)
8. Integration “smoke test” that:

   * loads a sample config
   * launches `sleep 999`
   * warns at thresholds
   * kills at expiry
   * emits events to a test client