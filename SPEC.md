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

**CrewList itself contains no intelligence.** It is a Rust CLI over two stores.
The reasoning lives in a *skill* the external agent runs; that skill drives this
CLI. This spec defines the data plane and the command contract that skill
depends on. The skill document is a separate deliverable.

### 1.1 The loop

```
  human                     crewlist (this tool)              external agent
  -----                     --------------------              --------------
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

### 1.2 Non-goals for v1

- No multi-user, no auth, no tenancy. One human.
- No LLM calls, prompts, or model config inside the Rust binary.
- No async job queue, worker daemon, or leases. `handoff` is a synchronous read.
- No task dependencies, recurrence, due dates, or reminders.
- No sync/export to external todo systems.

---

## 2. Technology

| Concern | Choice | Rationale |
|---|---|---|
| CLI | Rust 2021, `clap` v4 derive | Single static binary agents can shell out to |
| Task metadata | PostgreSQL 15+ | Relational: ids, status, parent/child, ordering |
| Task details | MongoDB 6+ | Free-form-*shaped* JSON, fixed schema enforced by validator |
| PG driver | `sqlx` (async, compile-time checked) | Migrations built in |
| Mongo driver | `mongodb` official crate | |
| Runtime | `tokio` | |
| Errors | `thiserror` (lib) + `anyhow` (bin) | |
| Tests | `assert_cmd` + `testcontainers` | Real PG/Mongo in integration tests |

### 2.1 Why two stores

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

The collection is created by `crewlist init` with a `$jsonSchema` validator at
`validationLevel: "strict"`, `validationAction: "error"`. The validator
requires `task_id`, `schema_version`, `created_at`, `updated_at`; pins types on
every field; and sets `additionalProperties: false` at the document root and
inside each array element. "Fixed schema" means the database rejects drift, not
that the application promises to behave.

Unique index on `task_id`.

### 5.3 Cross-store write order

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
fatal. Orphan reaping is deferred to a later `crewlist gc`.

---

## 6. CLI surface

```
crewlist [GLOBAL] <human|agent|init> <SUBCOMMAND>
```

### 6.1 Global

| Flag / env | Meaning |
|---|---|
| `--json` | Machine output on stdout. Errors also become JSON. |
| `--config <path>` | Default `~/.config/crewlist/config.toml` |
| `CREWLIST_POSTGRES_URL` | Overrides config |
| `CREWLIST_MONGO_URL` | Overrides config |
| `-q, --quiet` / `-v, --verbose` | Log level on **stderr** only |

Precedence: flag > env > config file > built-in default.

stdout carries data. stderr carries logs, progress, and errors. `--json` output
is a single JSON value with no leading or trailing prose, so the agent skill can
pipe it straight into a parser.

### 6.2 `crewlist init`

Runs pending Postgres migrations and creates the Mongo collection, validator,
and indexes. Idempotent — safe to run on an initialized store.

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
| 2 | Usage error (clap) |
| 3 | Task not found |
| 4 | Illegal state transition |
| 5 | Storage unavailable (PG or Mongo unreachable) |
| 6 | Validation failure (title length, bad `--parent`, schema rejection) |

Under `--json`, every non-zero exit also writes to stdout:

```json
{ "error": { "code": "illegal_transition", "message": "task 1 is 'done'; cannot hand off" } }
```

`code` values: `not_found`, `illegal_transition`, `validation`, `storage`,
`internal`. Stable strings — the skill branches on them.

---

## 7. Acceptance criteria

Each AC is one test. `test:` names the Rust test function. Prefix convention:
`unit_` = no I/O, `pg_`/`mongo_` = single-store integration, `cli_` = full
binary via `assert_cmd` against testcontainers.

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
| AC-48 | Mongo insert failure leaves **no** Postgres row | `integration_mongo_failure_leaves_no_row` |
| AC-49 | Postgres failure after a Mongo write leaves an orphan doc and no row — read paths stay correct | `integration_pg_failure_orphans_only` |
| AC-50 | A Mongo doc violating the validator is rejected, exit 6 | `mongo_validator_rejects_bad_doc` |
| AC-51 | A doc with unknown `schema_version` is rejected on read, not silently parsed | `unit_unknown_schema_version_rejected` |
| AC-52 | Postgres unreachable → exit 5 with `storage` code, no partial write | `cli_pg_down_exits_5` |
| AC-53 | Mongo unreachable → exit 5; commands needing no detail still work | `cli_mongo_down_degrades` |

### 7.10 Output contract

| # | Criterion | test |
|---|---|---|
| AC-54 | `--json` stdout parses as exactly one JSON value, no prose | `cli_json_stdout_is_pure` |
| AC-55 | Logs and progress go to stderr, never stdout | `cli_logs_on_stderr_only` |
| AC-56 | Every error path under `--json` emits the §6.5 error object | `cli_json_error_shape` |
| AC-57 | Error `code` strings match §6.5 exactly | `cli_error_codes_stable` |
| AC-58 | `crewlist init` twice in a row exits 0 both times | `cli_init_is_idempotent` |

### 7.11 End-to-end

| # | Criterion | test |
|---|---|---|
| AC-59 | The full §1.1 loop — add → list → handoff → add×2 → done — leaves 1 `done` root, 2 `todo` children, and an empty agent queue | `e2e_tree_service_walkthrough` |

---

## 8. Repository layout (proposed)

```
crewlist/
├── Cargo.toml                  # workspace
├── crates/
│   ├── crewlist-core/          # domain: Task, Status, transitions, validation
│   │   └── src/{task,status,detail,error}.rs
│   ├── crewlist-store/         # PgStore + MongoStore, write-order policy
│   │   └── migrations/0001_init.sql
│   └── crewlist-cli/           # clap surface, rendering, exit codes
├── tests/                      # cli_*, e2e_* via assert_cmd + testcontainers
└── SPEC.md
```

`crewlist-core` holds no I/O, which is what makes AC-1 … AC-7 and AC-51 fast
unit tests rather than container tests.

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
4. **`gc`.** Orphan detail-document reaping is deferred. Acceptable?
