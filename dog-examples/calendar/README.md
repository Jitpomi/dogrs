# Real-Time Collaborative Calendar (DogRS Example)

A complete, production-grade example demonstrating how to build a real-time, multi-tenant collaborative calendar application using **DogRS**.

This example combines:
- **`dog-core`**: Service lifecycle, multi-tenant context, and global/service hooks.
- **`dog-schema`**: Declarative request validation with automatic error formatting (422 Unprocessable Entity).
- **`dog-transport`**: Seamless Axum HTTP REST routing and real-time WebSocket broadcasting.
- **`dog-queue`**: Delayed background job scheduling (`run_at` / `scheduled`) with **cancel-wins** guarantees for event reminders.
- **Interactive Web UI**: A modern dashboard with live REST/WS indicators, instant multi-client synchronization, reminder toasts, and a real-time packet inspection console.

---

## ⚡ Architecture & Flow

```text
                                  ┌───────────────────────────┐
                                  │      Client Browser(s)    │
                                  │  (REST API + WebSockets)  │
                                  └─────────────┬─────────────┘
                                                │
                     ┌──────────────────────────┴──────────────────────────┐
                     │                     Axum Router                     │
                     │  - REST Service:  /api/events                       │
                     │  - WebSocket:     /ws                               │
                     │  - Static Assets: / (Dashboard UI)                  │
                     └─────────────┬─────────────────────────▲─────────────┘
                                   │                         │
                                   ▼                         │
                      ┌─────────────────────────┐            │
                      │   EventsService (CRUD)  │            │
                      │   - dog-schema validate │            │
                      │   - CalendarAdapter     │            │
                      └────────────┬────────────┘            │
                                   │                         │
         ┌─────────────────────────┴─────────────┐           │
         │ Emits lifecycle event (*.*)           │ Enqueue delayed reminder (start - 15m)
         ▼                                       ▼
┌─────────────────────────────────┐   ┌─────────────────────────────────┐
│     Broadcast Channels          │   │           dog-queue             │
│   (tokio::sync::broadcast)      │   │   - In-memory backend           │
│   - "event_channel" (cap: 128)  │   │   - Delayed execution (run_at)  │
└────────────────┬────────────────┘   │   - Cancel-wins on update/delete│
                 │                    └──────────────┬──────────────────┘
                 │                                   │ Fires when due:
                 │                                   │ sends "reminder" frame
                 │                                   │
                 ├───────────────────────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│      declare_ws_adapter!        │
│   Pushes BROADCAST frames to    │
│   all active WebSocket clients  │
└─────────────────────────────────┘
```

---

## 📂 Module Structure & Responsibilities

This application strictly follows the layout prescribed in [docs/application-structure.md](../../docs/application-structure.md):

| File | Responsibility |
|---|---|
| [`src/main.rs`](src/main.rs) | Executable entry point: loads environment variables, configures logging via `tracing`, builds application via `lib::build()`, merges Axum routes (`/api`, `/ws`, and static files), and manages graceful shutdown. |
| [`src/lib.rs`](src/lib.rs) | Reusable construction API: assembles `DogApp`, initializes `dog-queue` worker pool, instantiates in-memory state, and exposes `build()`. |
| [`src/config.rs`](src/config.rs) | Application configuration: loads environment variables (`.env`) for HTTP server settings (`HTTP_HOST`, `HTTP_PORT`) and WebSocket parameters (`WS_PUBLIC_BROADCASTS`, `WS_ALLOWED_ORIGINS`). |
| [`src/app.rs`](src/app.rs) | Application construction: applies configuration via `crate::config::config(&mut builder)` and registers global hooks and channels. |

| [`src/channels.rs`](src/channels.rs) | Real-time channel configuration: allocates a bounded 128-message `broadcast::channel`, registers it under `"event_channel"`, and attaches a wildcard `*.*` lifecycle listener to broadcast service mutations. |
| [`src/hooks.rs`](src/hooks.rs) | Application-wide hooks: implements `LogAround` (tracing incoming provider/path/method) and `LogAfter` (tracing operation success/failure). |
| [`src/background/mod.rs`](src/background/mod.rs) | Background job processing: implements `CalendarQueue`, defines `EventReminderJob`, and runs workers that broadcast `"reminder"` events when due. |
| [`src/services/mod.rs`](src/services/mod.rs) | Service registration: registers `EventsService` into the `DogAppBuilder` and mounts service hooks. |
| [`src/services/types.rs`](src/services/types.rs) | Shared domain types: defines `CalendarParams` (headers, query, method, path) and `CalendarState` (multi-tenant store and reminder job map). |
| [`src/services/adapters/calendar_adapter.rs`](src/services/adapters/calendar_adapter.rs) | Data access layer: provides multi-tenant CRUD (`_create`, `_find`, `_get`, `_patch`, `_remove`), delayed reminder enqueuing via `dog-queue`, and cancel-wins cancellation. |
| [`src/services/events/mod.rs`](src/services/events/mod.rs) | Service interface: exports `EventsService` and `EventParams`. |
| [`src/services/events/events_service.rs`](src/services/events/events_service.rs) | Service implementation: implements the `DogService` trait and delegates operations to `CalendarAdapter`. |
| [`src/services/events/events_schema.rs`](src/services/events/events_schema.rs) | Declarative schema validation: defines `CreateEvent` and `PatchEvent` input rules using `dog-schema`. |
| [`src/services/events/events_hooks.rs`](src/services/events/events_hooks.rs) | Service-specific hooks: defines `NormalizeEventsResult`. |
| [`src/services/events/events_shared.rs`](src/services/events/events_shared.rs) | Service capabilities and registration helper: wires schema validation and hooks. |
| [`src/services/events/event_params.rs`](src/services/events/event_params.rs) | Scoped parameter conversion from `CalendarParams`. |
| [`static/`](static/) | Interactive Web Dashboard (`index.html`, `style.css`, `app.js`). |
| [`tests/http.rs`](tests/http.rs) | Comprehensive integration tests covering CRUD, 422 validation, tenant isolation, live WebSockets, and `dog-queue` reminder scheduling / cancel-wins. |

---

## 🔑 Key Features Demonstrated

### 1. Real-Time WebSocket Synchronization
- All CRUD mutations (`created`, `updated`, `patched`, `removed`) emit DogRS service lifecycle events.
- In [`src/channels.rs`](src/channels.rs), a listener intercepts `*.*` mutations and sends them across a bounded Tokio broadcast channel.
- Axum's WebSocket adapter (`dog_transport::declare_ws_adapter!`) subscribes to this broadcast channel and pushes `BROADCAST` frames to all connected browsers.

### 2. Delayed Reminder Scheduling with Cancel-Wins
- When an event is scheduled with a future `start_time`, `CalendarAdapter` calculates `start_time - 15 minutes` and enqueues an `EventReminderJob` into `dog-queue` using `EnqueueOptions::scheduled(reminder_time)`.
- If an event is updated or deleted before the reminder fires, the adapter immediately cancels the queued job (`queue.adapter.cancel(ctx, job_id)`). This **cancel-wins** guarantee prevents phantom notifications.
- When the reminder becomes due, the worker executes and broadcasts a `"reminder"` frame over WebSockets.

### 3. Declarative Schema Validation
- Defined in [`src/services/events/events_schema.rs`](src/services/events/events_schema.rs) using `dog-schema`.
- Supports title, description, start/end timestamps, location, meeting URL, category, color, and **recurrence rules**.
- Missing required fields (such as omitting `title`) return a structured `422 Unprocessable Entity` JSON response before reaching business logic.

### 4. Recurring Event Cadences & Dynamic Expansion
- Events can optionally repeat using the `recurrence` field:
  - `none`: Does not repeat (single occurrence on specified day).
  - `daily`: Repeats every day including weekends.
  - `weekdays`: Repeats Monday through Friday (ideal for standups and team syncs).
  - `weekly`: Repeats every 7 days on the selected weekday.
  - `biweekly`: Repeats every 14 days (ideal for 1:1s and sprint planning).
  - `monthly`: Repeats on the same day of each month.
- **Dynamic View Expansion**: Rather than polluting the database with hundreds of redundant rows, recurring events are dynamically projected across the active viewing window (**Week**, **Day**, **Month**, **Agenda**, and **Mini Calendar**) with repeat badges (`🔁`) and ID deduplication.

### 5. Instant Video Meeting Integration
- One-click **`+ Google Meet`** button immediately configures meetings with [`https://meet.google.com/new`](https://meet.google.com/new), creating a valid, active Google Meet room without expired meeting code errors.
- Quick **Join Video Call** buttons in the left sidebar upcoming callout and event popover flyouts.

### 6. Multi-Tenant Isolation
- In-memory event storage is partitioned by tenant ID (`events_by_tenant: HashMap<String, HashMap<String, Value>>`).
- Passing the `x-tenant-id` header isolates events per tenant workspace.

---

## 🚀 Running the Application

### 1. Start the Server

```bash
cargo run -p calendar
```

Output:
```text
Listening on http://127.0.0.1:3036
  Calendar UI:  http://127.0.0.1:3036
  REST API:     POST/GET http://127.0.0.1:3036/api/events
  WebSocket:    ws://127.0.0.1:3036/ws
```

### 2. Open the Live Web Dashboard

Open `http://127.0.0.1:3036` in your browser. Open multiple tabs side-by-side to watch real-time two-way synchronization in action:
- **Multiple Views**: Switch between **Day**, **Week**, **Month**, and **Agenda** views (`1`, `2`, `3`, `4`).
- **Schedule Recurring Events**: Pick recurrence cadences (Daily, Weekdays, Weekly, Bi-weekly, Monthly, or fully **Custom** intervals with days-of-week and termination rules) with dynamic day labels.
- **One-Click Video Calls**: Click `+ Google Meet` in the creation modal or click `Join Video Call` from any meeting card.
- **Instant Sync**: All other browser tabs immediately reflect new, updated, or removed events via WebSocket without reloading.
- **Keyboard Shortcuts**:
  - <kbd>C</kbd> — Create new event
  - <kbd>T</kbd> — Jump to today
  - <kbd>/</kbd> or <kbd>⌘K</kbd> — Universal search
  - <kbd>1</kbd> / <kbd>2</kbd> / <kbd>3</kbd> / <kbd>4</kbd> — Switch view (Day / Week / Month / Agenda)
  - <kbd>Esc</kbd> — Close modals and popovers
- **Packet Console**: Inspects raw incoming and outgoing transport packets in real-time.

---

## 📡 REST API & WebSocket Usage

### Create a Recurring Event
```bash
curl -i -X POST http://127.0.0.1:3036/api/events \
  -H "content-type: application/json" \
  -d '{
    "title": "Daily Engineering Standup",
    "description": "Sync on blockers and DogRS architecture",
    "start_time": "2026-10-26T09:30:00Z",
    "end_time": "2026-10-26T10:00:00Z",
    "recurrence": "weekdays",
    "meeting_url": "https://meet.google.com/new",
    "location": "Remote",
    "color": "indigo"
  }'
```

### List Events
```bash
curl -i http://127.0.0.1:3036/api/events
```

### Tenant-Isolated Requests
```bash
# Tenant A
curl -i -X POST http://127.0.0.1:3036/api/events \
  -H "x-tenant-id: tenant-a" \
  -H "content-type: application/json" \
  -d '{"title":"Tenant A Private Meeting"}'

# Tenant B (cannot see Tenant A events)
curl -i http://127.0.0.1:3036/api/events \
  -H "x-tenant-id: tenant-b"
```

### WebSocket Broadcast Frame Format
When any client mutates an event, all subscribed clients receive:
```json
{
  "type": "BROADCAST",
  "event": "created",
  "payload": {
    "id": "event:b91cf6f2-8f15-46aa-b2b9-e1f96cb130bb",
    "title": "Daily Engineering Standup",
    "recurrence": "weekdays",
    "meeting_url": "https://meet.google.com/new",
    "start_time": "2026-10-26T09:30:00Z",
    "end_time": "2026-10-26T10:00:00Z"
  }
}
```

### WebSocket Reminder Frame Format
When a delayed reminder job fires:
```json
{
  "type": "BROADCAST",
  "event": "reminder",
  "payload": {
    "event_id": "event:b91cf6f2-8f15-46aa-b2b9-e1f96cb130bb",
    "title": "Daily Engineering Standup",
    "start_time": "2026-10-26T09:30:00Z",
    "message": "Upcoming Event Reminder: Daily Engineering Standup"
  }
}
```

---

## 🧪 Testing & Validation

### Run Integration Tests
```bash
cargo test -p calendar
```
All 7 tests verify:
1. `events_create_missing_title_is_422`: Schema validation failure.
2. `events_create_and_get`: Creation, ID generation, and retrieval.
3. `events_create_with_recurrence`: Recurrence rule validation and persistence.
4. `events_patch_and_delete`: Partial updates and removal.
5. `events_are_isolated_by_tenant`: Tenant boundary isolation.
6. `events_realtime_websocket_broadcast`: Live WebSocket message delivery over TCP.
7. `events_queue_reminder_and_cancel_wins`: Delayed job enqueuing and cancel-wins on deletion.

### Check Application Structure Compliance
```bash
python3 dog-examples/check_structure.py
```

### Check Clippy and Formatting
```bash
cargo clippy -p calendar --all-targets
cargo fmt --all -- --check
```

