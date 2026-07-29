# CrewList — Specification v1

Status: **draft, pending review.** No code is written against this yet.

---

## 1. What CrewList is

CrewList is a single-user todo list with a machine-facing side door.

A human captures an intent (`find a reliable tree removal service`). An external
AI agent — OpenClaw, Hermes, Codex, Claude Code — picks that task up, does the
real work with its own capabilities (search, phone lookup, comparison), and
writes concrete, human-actionable items back into the same list
(`Call Alex's Tree Service 617-898-0989`).

**CrewList itself contains no intelligence.** It is a Rust CLI talking to a Rust
server that owns two stores. The reasoning lives in a *skill* the external agent
runs; that skill drives the CLI. This spec defines the data plane and the
command contract that skill depends on. The skill document is a separate
deliverable.

### 1.1 The loop

```
  human                     crewlist CLI ──▶ server            external agent
  -----                     ----------------------            --------------
  crewlist human add "…" ──▶ INSERT task ── id 1
                                                    ◀── crewlist agent list
                                                        [{"id":1,"title":"…"}]
                                                    ◀── crewlist agent handoff 1
                             status → handed_off ──▶ {task, detail, children}

                                                        (agent does real work:
                                                         search, vet, compare)

                                                    ◀── crewlist agent add --parent 1 "Call Alex's…"
                                                    ◀── crewlist agent add --parent 1 "Get 2nd quote…"
                                                    ◀── crewlist agent done 1 --summary "3 vetted options"
  crewlist human list      ◀── 1 done, children todo
```

Every command above is one HTTP round trip to the server. The CLI holds no
database connection and no domain rules it could get wrong on its own.

### 1.2 Non-goals for v1

- No multi-user, no auth, no tenancy. One human. The server binds loopback
  only and trusts every caller that can reach it (§2.3).
- No LLM calls, prompts, or model config in either binary.
- No public HTTP API. The wire protocol is internal and unversioned (§6.6) —
  the CLI is the only sanctioned interface, and agents use it by shelling out.
- No async job queue, worker daemon, or leases. `handoff` is a synchronous read.
- No task dependencies, recurrence, due dates, or reminders.
- No sync/export to external todo systems.

---

## 2. Technology

| Concern | Choice | Rationale |
|---|---|---|
| CLI | Rust 2021, `clap` v4 derive | Single static binary agents shell out to |
| CLI transport | `reqwest` (blocking) + `serde_json` | No async runtime, no DB drivers, fast startup |
| Server | Rust 2021, `axum` + `tokio` | Owns both stores and all domain rules |
| Task metadata | PostgreSQL 15+ | Relational: ids, status, parent/child, ordering |
| Task details | MongoDB 6+ | Free-form-*shaped* JSON, fixed schema enforced by validator |
| PG driver | `sqlx` (async, compile-time checked) | Migrations built in |
| Mongo driver | `mongodb` official crate | |
| Errors | `thiserror` (libs) + `anyhow` (bins) | |
| Deployment | Docker Compose: postgres, mongo, server | `docker compose up` is the whole backend |
| Tests | `assert_cmd` + `testcontainers` | Real PG/Mongo/server in integration tests |

### 2.1 Process topology

```
  host                                docker compose network
  ────                                ──────────────────────
  crewlist CLI                ┌──────────────────────────────┐
       │                      │                              │
       │  HTTP/JSON           │   crewlist-server            │
       └──────────────────────┼──▶ 127.0.0.1:8787            │
                              │        │            │        │
                              │        ▼            ▼        │
                              │   postgres:5432  mongo:27017 │
                              │   (crewlist)     (crewlist)  │
                              └──────────────────────────────┘
```

The server publishes `127.0.0.1:8787:8787` — bound to host loopback, not
`0.0.0.0`. Postgres and Mongo publish **no** host ports at all; they are
reachable only from the server on the compose network. The blast radius of a
misconfiguration is therefore one process on one machine.

### 2.2 Why a server, and what it costs

The CLI could talk to both databases directly, and for a single-user todo list
that would be defensible. It loses on three counts:

1. **Credential sprawl.** OpenClaw, Hermes, Codex, and Claude Code each run in
   their own sandbox. Direct access means every one of them holds Postgres
   *and* Mongo credentials and can reach both ports. With a server they need
   one URL.
2. **One copy of the hard part.** The cross-store write-ordering rule (§5.3) is
   the most breakable thing in this design. In the server it exists once. In a
   direct-access CLI it exists in every installed copy, at whatever version
   that copy happens to be.
3. **Startup cost.** `sqlx` plus the Mongo driver plus `tokio` in a binary that
   an agent invokes dozens of times per task is real latency. A blocking HTTP
   client is not.

The cost is honest and worth stating: two processes instead of one, nothing
works when the server is down, and this spec grows a wire protocol plus an HTTP
status → exit code mapping (§6.6). For a tool whose entire purpose is to be
driven by sandboxed agents, that trade is worth making.

### 2.3 Trust model

There is none. The server has no authentication, no authorization, and no rate
limiting; anything that can open a socket to `127.0.0.1:8787` has full control
of the task list. This is safe precisely and only because of the loopback bind.

**Consequence worth knowing before you build on it:** an agent running in its
own container cannot reach the server without host networking or an explicit
port mapping. If agents move off-host, this decision has to be revisited —
adding a bearer token is a middleware layer and one new exit code, not a
redesign, so the seam is cheap to leave alone until then.

### 2.4 Why two stores

Postgres owns **existence, identity, status, and hierarchy** — everything you
filter, sort, or transition on. Mongo owns the **payload**: description, agent
notes, sources, contacts. The split is enforced: no query in this spec filters
on a Mongo field, and no Postgres column holds free-form agent prose except the
short `summary` denormalization.

---

## 3. Domain model

### 3.1 Task

A task is a single unit of work. Tasks form a **strictly two-level** hierarchy:
root tasks (what the human asked for) and children (what the agent determined
must actually be done). Children may not have children.

| Field | Type | Notes |
|---|---|---|
| `id` | i64 | Globally unique, monotonic, human-typeable. Displayed as-is. |
| `title` | String | 1–500 chars after trimming |
| `status` | enum | `todo` \| `handed_off` \| `done` \| `cancelled` |
| `origin` | enum | `human` \| `agent` — who created it |
| `parent_id` | Option\<i64\> | `None` for roots |
| `agent_eligible` | bool | May the agent queue pick this up |
| `detail_id` | Option\<String\> | Mongo ObjectId hex; `None` until a detail doc exists |
| `summary` | Option\<String\> | Agent's closing one-liner, denormalized for `list` |
| `created_at` / `updated_at` | timestamptz | |
| `handed_off_at` / `completed_at` | Option\<timestamptz\> | |

### 3.2 Status lifecycle

```
                 agent handoff
        ┌──────────────────────────────┐
        ▼                              │
     handed_off ──── agent skip ───▶ todo ◀─── human reopen ───┐
        │                              │                       │
        │ agent done                   │ human done            │
        │ human done                   │                       │
        ▼                              ▼                       │
       done ◀────────────────────────────────────────────────  │
        └───────────────────────────────────────────────────────┘

     todo | handed_off ── human cancel ──▶ cancelled  (terminal)
```

Legal transitions, and nothing else:

| From | To | Trigger |
|---|---|---|
| `todo` | `handed_off` | `agent handoff` |
| `todo` | `done` | `human done` |
| `todo` | `cancelled` | `human cancel` |
| `handed_off` | `todo` | `agent skip` |
| `handed_off` | `done` | `agent done`, `human done` |
| `handed_off` | `cancelled` | `human cancel` |
| `done` | `todo` | `human reopen` |
| `cancelled` | — | terminal; `human rm` only |

Any other transition is an error (exit code 4) and mutates nothing.

**`done` on a root means the agent's work is done, not that the errand is
finished.** Task 1 (`find a tree service`) goes `done` when the agent has
delivered its findings; its children stay `todo` until the human makes the
calls. `human list` therefore shows done parents with open children, and this
is correct, not a bug. Completing a parent never cascades to children.

### 3.3 The agent queue

`agent list` returns tasks matching **all** of:

- `status = 'todo'`
- `origin = 'human'`
- `agent_eligible = true`
- `parent_id IS NULL`

The `origin = 'human'` clause is what keeps the loop from eating itself:
`Call Alex's Tree Service 617-898-0989` is work for the human, so it never
re-enters the agent queue. `--all` overrides the filter for debugging.

`human add --self` sets `agent_eligible = false` for things the human never
wants an agent touching (`buy milk`).

---

## 4. Postgres schema

Migration `0001_init.sql`:

```sql
CREATE TYPE task_status AS ENUM ('todo', 'handed_off', 'done', 'cancelled');
CREATE TYPE task_origin AS ENUM ('human', 'agent');

CREATE TABLE tasks (
    id              BIGSERIAL PRIMARY KEY,
    title           TEXT        NOT NULL
                    CHECK (char_length(btrim(title)) BETWEEN 1 AND 500),
    status          task_status NOT NULL DEFAULT 'todo',
    origin          task_origin NOT NULL,
    parent_id       BIGINT      REFERENCES tasks(id) ON DELETE CASCADE,
    agent_eligible  BOOLEAN     NOT NULL DEFAULT TRUE,
    detail_id       TEXT,
    summary         TEXT        CHECK (summary IS NULL OR char_length(summary) <= 2000),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    handed_off_at   TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,

    CONSTRAINT no_self_parent CHECK (parent_id IS DISTINCT FROM id)
);

CREATE INDEX tasks_queue_idx  ON tasks (status, origin, agent_eligible)
                              WHERE parent_id IS NULL;
CREATE INDEX tasks_parent_idx ON tasks (parent_id);
```

Two-level depth is enforced in the application layer, not by a trigger: `agent
add --parent X` fails if `X.parent_id IS NOT NULL`. Rationale — the error needs
a readable message and a specific exit code, which a CHECK constraint cannot
give.

`updated_at` is set by the application on every mutating statement (not a
trigger), so it stays testable without a live database.

---

## 5. MongoDB detail document

Database `crewlist`, collection `task_details`, one document per task that has
details. Tasks with no details have `detail_id = NULL` and **no document** —
absence is normal, not an error.

### 5.1 Shape

```json
{
  "_id": ObjectId("…"),
  "task_id": 1,
  "schema_version": 1,
  "description": "Big oak in the back yard, leaning toward the garage.",
  "notes": [
    { "author": "codex", "body": "Checked 4 local services", "at": ISODate("…") }
  ],
  "sources": [
    { "url": "https://…", "title": "MA arborist registry", "retrieved_at": ISODate("…") }
  ],
  "contacts": [
    { "name": "Alex's Tree Service", "phone": "617-898-0989", "email": null, "url": null }
  ],
  "summary": "3 vetted options; Alex cheapest and insured.",
  "created_at": ISODate("…"),
  "updated_at": ISODate("…")
}
```

Every array defaults to `[]`. `description` and `summary` default to `""` and
`null`. `schema_version` is `1` for all v1 documents; readers must reject
unknown versions rather than guess.

### 5.2 Enforcement

The collection is created by the **server on boot** with a `$jsonSchema`
validator at `validationLevel: "strict"`, `validationAction: "error"`. The validator
requires `task_id`, `schema_version`, `created_at`, `updated_at`; pins types on
every field; and sets `additionalProperties: false` at the document root and
inside each array element. "Fixed schema" means the database rejects drift, not
that the application promises to behave.

Unique index on `task_id`.

### 5.3 Cross-store write order

This rule lives entirely in the server. Clients never see it, cannot violate
it, and hold no partial-write recovery logic.

There is no distributed transaction. The rule is **Mongo first, Postgres
second**:

1. Insert or update the Mongo detail document; obtain its `ObjectId`.
2. In a single Postgres transaction, insert/update the row carrying `detail_id`.

If step 2 fails, the Mongo document is orphaned — invisible, because nothing
references it, and harmless. If step 1 fails, nothing is written anywhere.
The failure mode is therefore always *garbage*, never *a task whose details
silently vanished*. Postgres is the sole authority on whether a task exists.

Deletes reverse it: delete the Postgres row (children cascade), then
best-effort delete the Mongo documents. A failed Mongo delete is logged, not
fatal. Orphan reaping is deferred to a later server-side `gc`.

---

## 6. CLI surface

```
crewlist [GLOBAL] <human|agent|health> <SUBCOMMAND>
```

### 6.1 Global

| Flag / env | Meaning |
|---|---|
| `--json` | Machine output on stdout. Errors also become JSON. |
| `--config <path>` | Default `~/.config/crewlist/config.toml` |
| `--server <url>` / `CREWLIST_SERVER_URL` | Default `http://127.0.0.1:8787` |
| `--timeout <secs>` | Request timeout, default 30 |
| `-q, --quiet` / `-v, --verbose` | Log level on **stderr** only |

Precedence: flag > env > config file > built-in default. `CREWLIST_SERVER_URL`
is the only setting an agent ever needs, and in the default deployment it needs
none.

The server takes `CREWLIST_POSTGRES_URL`, `CREWLIST_MONGO_URL`, and
`CREWLIST_BIND` (default `127.0.0.1:8787`) from its own environment. **No
database URL is ever read by the CLI** — if a client-side config key looks like
a connection string, that is a bug.

stdout carries data. stderr carries logs, progress, and errors. `--json` output
is a single JSON value with no leading or trailing prose, so the agent skill can
pipe it straight into a parser.

### 6.2 Schema setup and `crewlist health`

There is no `crewlist init`. The **server** runs pending Postgres migrations and
creates the Mongo collection, validator, and indexes during boot, before it
begins listening. Startup is idempotent, so a restart against an initialized
store is a no-op. A server that cannot complete migration refuses to serve and
exits non-zero rather than accepting traffic against a half-built schema.

`crewlist health` is the client-side counterpart: it calls the server's health
endpoint and reports reachability plus each store's status. It is the one
command an agent should run when something looks wrong.

```
$ crewlist health
server    http://127.0.0.1:8787   ok (0.4.0)
postgres  ok
mongo     ok
```

Exit 0 when everything is `ok`, exit 5 otherwise.

### 6.3 Human commands

| Command | Behavior |
|---|---|
| `human add <title> [--detail <text>] [--detail-file <path>] [--self]` | Creates a root task, `origin=human`, `status=todo`. Prints the new id. |
| `human list [--status <s>] [--all] [--tree]` | Default: open work (`todo`, `handed_off`), children indented under parents. `--all` includes `done` and `cancelled`. |
| `human show <id>` | Full task: metadata, detail document, children. |
| `human done <id>` | → `done`, stamps `completed_at`. |
| `human reopen <id>` | `done` → `todo`, clears `completed_at`. |
| `human cancel <id>` | → `cancelled`. |
| `human rm <id>` | Hard delete. Children cascade. Requires `--force` if the task has children. |

Default `human list` output:

```
  1  find a reliable tree removal service        handed_off
  2    Call Alex's Tree Service 617-898-0989     todo
  3    Get 2nd quote: Greenline Arborists        todo
  4  buy milk                                    todo
```

Ids are global and stable. Children are shown by indentation, not by dotted
`1.1` notation — there is exactly one id namespace, so `crewlist human done 2`
is never ambiguous.

### 6.4 Agent commands

| Command | Behavior |
|---|---|
| `agent list [--all]` | The agent queue (§3.3). JSON by default for this subcommand. |
| `agent handoff <id>` | Returns the full task payload; sets `status=handed_off`, stamps `handed_off_at`. |
| `agent add --parent <id> <title> [--detail <text>] [--source <url>]…` | Creates a child, `origin=agent`, `status=todo`. Prints the new id. Repeatable per discovered action. |
| `agent done <id> --summary <text> [--source <url>]…` | → `done`, writes `summary` to both stores, appends sources. |
| `agent skip <id> --reason <text>` | `handed_off` → `todo`. Records the reason as a note. Returns the task to the queue. |

`agent handoff` is a **read with a status stamp** — no lock, no lease, no
exclusivity. A second `handoff 1` while the first agent is still working
succeeds and returns the same payload. With one human and one agent runtime at a
time this is the right trade; adding leases later is additive (new columns, new
exit code) and does not change any contract defined here.

`agent handoff` payload — this is the skill's input contract:

```json
{
  "task": {
    "id": 1,
    "title": "find a reliable tree removal service",
    "status": "handed_off",
    "origin": "human",
    "parent_id": null,
    "agent_eligible": true,
    "summary": null,
    "created_at": "2026-07-28T14:02:11Z",
    "updated_at": "2026-07-28T14:09:33Z",
    "handed_off_at": "2026-07-28T14:09:33Z",
    "completed_at": null
  },
  "detail": {
    "schema_version": 1,
    "description": "Big oak in the back yard, leaning toward the garage.",
    "notes": [],
    "sources": [],
    "contacts": [],
    "summary": null
  },
  "children": []
}
```

When a task has no detail document, `detail` is the fully-defaulted object
above (empty arrays, `""` description) — **never `null`**, so the skill needs no
null branch.

### 6.5 Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Unexpected internal error |
| 2 | Usage error (clap) — never reaches the server |
| 3 | Task not found |
| 4 | Illegal state transition |
| 5 | Backend unavailable — server unreachable, or a store is down |
| 6 | Validation failure (title length, bad `--parent`, schema rejection) |

Under `--json`, every non-zero exit also writes to stdout:

```json
{ "error": { "code": "illegal_transition", "message": "task 1 is 'done'; cannot hand off" } }
```

`code` values: `not_found`, `illegal_transition`, `validation`, `storage`,
`unreachable`, `internal`. Stable strings — the skill branches on them, and they
stay stable even though the wire protocol underneath does not.

**Exit codes are the contract, not HTTP status codes.** An agent must never need
to know a request was made. The mapping:

| Server response | `code` | Exit |
|---|---|---|
| `200 OK` / `201 Created` | — | 0 |
| `400 Bad Request` | `validation` | 6 |
| `404 Not Found` | `not_found` | 3 |
| `409 Conflict` | `illegal_transition` | 4 |
| `422 Unprocessable Entity` | `validation` | 6 |
| `500 Internal Server Error` | `internal` | 1 |
| `503 Service Unavailable` | `storage` | 5 |
| connection refused, DNS failure, timeout | `unreachable` | 5 |

A refused connection produces a message naming the URL and how to fix it, since
"is the server running" is the single most likely failure in practice:

```
error: cannot reach crewlist server at http://127.0.0.1:8787
       start it with `docker compose up -d`
```

### 6.6 Wire protocol (internal)

The CLI and server speak JSON over HTTP. **This protocol is internal and
unversioned**; it may change in any release. Nothing outside this repository
should depend on it, the skill document must never teach it, and an agent that
calls it directly is unsupported. The stable contracts are the CLI's arguments,
its `--json` output, and its exit codes.

Routes exist roughly one-to-one with commands — `POST /tasks`,
`GET /tasks?queue=agent`, `POST /tasks/{id}/handoff`, `POST /tasks/{id}/done`,
and so on, plus `GET /health`. They are documented in the server crate, not
here, because pinning them in the spec would create exactly the external
dependency this section rules out.

Server errors share the §6.5 body shape, so the CLI maps rather than
translates:

```json
{ "error": { "code": "not_found", "message": "task 42 not found" } }
```

---

## 7. Acceptance criteria

Each AC is one test. `test:` names the Rust test function. Prefix convention:

| Prefix | Scope |
|---|---|
| `unit_` | No I/O. Domain crate only. |
| `pg_` / `mongo_` | Server logic against one real store (testcontainers). |
| `srv_` | Server over HTTP, both stores up. |
| `cli_` | Full binary via `assert_cmd` against a live server. |
| `e2e_` | The §1.1 loop end to end. |

`cli_` tests get a fixture that boots Postgres, Mongo, and the server, then
hands the binary a `CREWLIST_SERVER_URL`. Tests that assert on transport
failure (AC-52, AC-53, AC-60) manipulate that fixture rather than mocking.

### 7.1 Status machine (pure, no I/O)

| # | Criterion | test |
|---|---|---|
| AC-1 | Every transition in §3.2's table is accepted | `unit_legal_transitions_accepted` |
| AC-2 | Every transition absent from that table is rejected | `unit_illegal_transitions_rejected` |
| AC-3 | `cancelled` accepts no outgoing transition | `unit_cancelled_is_terminal` |
| AC-4 | Rejected transitions return `IllegalTransition` carrying both states | `unit_rejection_names_both_states` |

### 7.2 Validation

| # | Criterion | test |
|---|---|---|
| AC-5 | Empty or whitespace-only title rejected, exit 6 | `cli_add_rejects_blank_title` |
| AC-6 | Title of 501 chars rejected; 500 accepted | `cli_add_title_length_boundary` |
| AC-7 | Titles are trimmed before storage | `unit_title_is_trimmed` |
| AC-8 | `--detail` and `--detail-file` together is a usage error, exit 2 | `cli_detail_flags_are_exclusive` |
| AC-9 | `--detail-file` on a missing path exits 6, creates nothing | `cli_detail_file_missing_is_clean` |

### 7.3 `human add`

| # | Criterion | test |
|---|---|---|
| AC-10 | Prints only the new id on stdout when not `--json` | `cli_human_add_prints_id` |
| AC-11 | Creates row with `origin=human`, `status=todo`, `parent_id=NULL` | `pg_human_add_defaults` |
| AC-12 | Without `--detail`, no Mongo doc is created and `detail_id IS NULL` | `mongo_no_detail_no_document` |
| AC-13 | With `--detail`, a doc is created and `detail_id` matches its `_id` | `mongo_detail_linked_to_row` |
| AC-14 | `--self` sets `agent_eligible=false` | `pg_self_flag_excludes_from_queue` |
| AC-15 | Ids are monotonically increasing across adds | `pg_ids_are_monotonic` |

### 7.4 Agent queue

| # | Criterion | test |
|---|---|---|
| AC-16 | `agent list` returns only `todo` + `origin=human` + eligible + root | `cli_agent_list_queue_filter` |
| AC-17 | Agent-created children never appear in `agent list` | `cli_agent_children_not_requeued` |
| AC-18 | `--self` tasks never appear in `agent list` | `cli_agent_list_skips_self_tasks` |
| AC-19 | `handed_off` tasks are absent from the queue | `cli_agent_list_excludes_handed_off` |
| AC-20 | Empty queue emits `[]` and exit 0 — not an error | `cli_agent_list_empty_is_ok` |
| AC-21 | `agent list` is JSON by default, without `--json` | `cli_agent_list_defaults_to_json` |

### 7.5 `agent handoff`

| # | Criterion | test |
|---|---|---|
| AC-22 | Returns the §6.4 payload with all three top-level keys | `cli_handoff_payload_shape` |
| AC-23 | Sets `status=handed_off` and stamps `handed_off_at` | `pg_handoff_sets_status` |
| AC-24 | Payload `status` reflects the **post**-transition value | `cli_handoff_reports_new_status` |
| AC-25 | Task with no detail doc yields defaulted `detail`, never `null` | `cli_handoff_detail_defaults` |
| AC-26 | Existing children are included in `children` | `cli_handoff_includes_children` |
| AC-27 | Handing off a `done` task exits 4 and mutates nothing | `cli_handoff_done_task_rejected` |
| AC-28 | Unknown id exits 3 | `cli_handoff_unknown_id` |
| AC-29 | A second `handoff` on a `handed_off` task exits 4 (only `todo` is handoff-able) | `cli_handoff_twice_rejected` |

### 7.6 `agent add`

| # | Criterion | test |
|---|---|---|
| AC-30 | Creates child with `origin=agent`, `status=todo`, correct `parent_id` | `pg_agent_add_child` |
| AC-31 | `--parent` pointing at a child exits 6 (two-level limit) | `cli_agent_add_rejects_grandchild` |
| AC-32 | `--parent` pointing at an unknown id exits 3 | `cli_agent_add_unknown_parent` |
| AC-33 | Repeated `--source` values all land in the child's `sources[]` | `mongo_agent_add_sources` |
| AC-34 | Parent's status is unchanged by `agent add` | `pg_agent_add_leaves_parent` |

### 7.7 `agent done` / `agent skip`

| # | Criterion | test |
|---|---|---|
| AC-35 | `agent done` sets `done`, stamps `completed_at`, writes `summary` to both stores | `cli_agent_done_writes_summary` |
| AC-36 | `agent done` without `--summary` is a usage error, exit 2 | `cli_agent_done_requires_summary` |
| AC-37 | `agent done` on a parent leaves children `todo` | `pg_done_does_not_cascade` |
| AC-38 | `agent skip` returns `handed_off` → `todo` and the task reappears in `agent list` | `cli_agent_skip_requeues` |
| AC-39 | `agent skip` appends a note carrying the reason | `mongo_skip_records_reason` |
| AC-40 | `agent skip` on a `todo` task exits 4 | `cli_agent_skip_wrong_state` |

### 7.8 Human read & lifecycle

| # | Criterion | test |
|---|---|---|
| AC-41 | Default `human list` hides `done` and `cancelled`; `--all` shows them | `cli_human_list_default_filter` |
| AC-42 | Children render indented directly beneath their parent | `cli_human_list_tree_order` |
| AC-43 | A `done` parent with `todo` children renders both (§3.2) | `cli_human_list_done_parent_open_children` |
| AC-44 | `human show` on a task with no Mongo doc succeeds with empty details | `cli_show_tolerates_missing_detail` |
| AC-45 | `human reopen` clears `completed_at` | `pg_reopen_clears_completed_at` |
| AC-46 | `human rm` on a parent without `--force` exits 6 and deletes nothing | `cli_rm_parent_needs_force` |
| AC-47 | `human rm --force` cascades children in Postgres | `pg_rm_cascades` |

### 7.9 Cross-store integrity

| # | Criterion | test |
|---|---|---|
| AC-48 | Mongo insert failure leaves **no** Postgres row | `srv_mongo_failure_leaves_no_row` |
| AC-49 | Postgres failure after a Mongo write leaves an orphan doc and no row — read paths stay correct | `srv_pg_failure_orphans_only` |
| AC-50 | A Mongo doc violating the validator is rejected, surfacing as exit 6 | `mongo_validator_rejects_bad_doc` |
| AC-51 | A doc with unknown `schema_version` is rejected on read, not silently parsed | `unit_unknown_schema_version_rejected` |
| AC-52 | Postgres down → server returns 503, CLI exits 5 with `storage`, nothing written | `cli_pg_down_exits_5` |
| AC-53 | Mongo down → exit 5; commands needing no detail still succeed | `cli_mongo_down_degrades` |

### 7.10 Output contract

| # | Criterion | test |
|---|---|---|
| AC-54 | `--json` stdout parses as exactly one JSON value, no prose | `cli_json_stdout_is_pure` |
| AC-55 | Logs and progress go to stderr, never stdout | `cli_logs_on_stderr_only` |
| AC-56 | Every error path under `--json` emits the §6.5 error object | `cli_json_error_shape` |
| AC-57 | Error `code` strings match §6.5 exactly | `cli_error_codes_stable` |
| AC-58 | Every §6.5 HTTP status maps to its specified exit code | `cli_status_to_exit_code_mapping` |

### 7.11 Client/server

| # | Criterion | test |
|---|---|---|
| AC-59 | Booting the server twice against the same stores exits 0 both times; migrations and the Mongo validator are idempotent | `srv_boot_is_idempotent` |
| AC-60 | Server unreachable → exit 5, `unreachable`, and a message naming the URL | `cli_unreachable_server_exit_5` |
| AC-61 | A server that cannot migrate refuses to listen and exits non-zero | `srv_failed_migration_refuses_traffic` |
| AC-62 | Server binds loopback by default; `CREWLIST_BIND` overrides it | `srv_binds_loopback_by_default` |
| AC-63 | `crewlist health` reports server + both stores, exit 0 when all ok | `cli_health_reports_all_ok` |
| AC-64 | `crewlist health` exits 5 when any store is down, naming which | `cli_health_names_failed_store` |
| AC-65 | The CLI reads no database URL from any source — config, env, or flag | `cli_holds_no_db_config` |
| AC-66 | `--timeout` is honored; a hung server yields exit 5, not a hang | `cli_timeout_exits_5` |

AC-65 is a guardrail, not a feature: it fails if anyone reintroduces direct
database access to the client.

### 7.12 End-to-end

| # | Criterion | test |
|---|---|---|
| AC-67 | The full §1.1 loop — add → list → handoff → add×2 → done — leaves 1 `done` root, 2 `todo` children, and an empty agent queue | `e2e_tree_service_walkthrough` |
| AC-68 | The same loop runs against a Compose-started backend, not just testcontainers | `e2e_against_compose_stack` |

---

## 8. Repository layout (proposed)

```
crewlist/
├── Cargo.toml                  # workspace
├── docker-compose.yml          # postgres, mongo, crewlist-server
├── Dockerfile                  # multi-stage build of crewlist-server
├── crates/
│   ├── crewlist-core/          # domain + wire types. NO I/O.
│   │   └── src/{task,status,detail,dto,error}.rs
│   ├── crewlist-store/         # PgStore + MongoStore, write-order policy
│   │   └── migrations/0001_init.sql
│   ├── crewlist-server/        # axum routes, boot migrations, error → status
│   ├── crewlist-client/        # blocking HTTP client, status → CrewError
│   └── crewlist-cli/           # clap surface, rendering, exit codes
├── tests/                      # cli_*, e2e_* via assert_cmd + testcontainers
└── SPEC.md
```

Dependency direction, which the workspace must enforce:

```
  cli ──▶ client ──▶ core ◀── store ◀── server ──▶ core
```

- **`crewlist-core` holds no I/O.** That is what keeps AC-1 … AC-7 and AC-51
  fast unit tests instead of container tests, and it is the only crate both
  sides share — so the status machine and the wire types cannot drift.
- **`crewlist-cli` must not depend on `crewlist-store`.** If that edge ever
  appears, database drivers are back in the client and AC-65 fails. This is the
  architectural invariant of the whole design; enforce it in CI, not by
  vigilance.

### 8.1 Deployment

`docker compose up -d` starts everything. The composition:

| Service | Image | Ports | Notes |
|---|---|---|---|
| `postgres` | `postgres:16-alpine` | none published | named volume, healthcheck |
| `mongo` | `mongo:7` | none published | named volume, healthcheck |
| `server` | built from `Dockerfile` | `127.0.0.1:8787:8787` | `depends_on` both healthchecks |

The CLI is installed on the host (`cargo install --path crates/crewlist-cli`)
and is **not** containerized — agents shell out to it, so it has to live where
the agent's shell lives.

---

## 9. Open questions

1. **Ordering.** `human list` currently implies `ORDER BY id`. Should there be
   manual reordering or priority? Not specced — assumed no.
2. **`agent note`.** The agent can attach sources via `agent add`/`agent done`
   and a reason via `agent skip`, but has no way to record free-form progress
   mid-task. Deliberate omission; add only if the skill turns out to need it.
3. **Contacts.** `contacts[]` exists in the detail schema but no v1 command
   populates it — the phone number lives in the child task's title
   (`Call Alex's Tree Service 617-898-0989`), which is what the human reads.
   Keep the field reserved, or drop it from v1?
4. **`gc`.** Orphan detail-document reaping is deferred. Acceptable? It is now
   a server-side concern — a scheduled task or admin route, not a CLI command.
5. **Containerized agents.** Loopback-only binding (§2.3) means an agent in its
   own container cannot reach the server without host networking. Will every
   agent runtime run on the host? If not, this is the decision to revisit
   first, and the fix is a bearer token plus a wider bind.
6. **Server lifecycle.** Nothing here says who starts the server. Is
   `docker compose up -d` run by hand, or should the CLI detect a dead server
   and offer to start it? The latter is convenient and a little magic.
