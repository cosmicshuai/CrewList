# CrewList

A single-user todo list with a machine-facing side door.

You capture an intent — `find a reliable tree removal service`. An external AI
agent (OpenClaw, Hermes, Codex, Claude Code) picks the task up, does the real
work with its own capabilities, and writes concrete, actionable items back into
your list — `Call Alex's Tree Service 617-898-0989`.

CrewList itself contains no intelligence. It is a Rust CLI talking to a Rust
server that owns PostgreSQL (task metadata) and MongoDB (task details). The
reasoning lives in a skill the external agent runs; that skill drives the CLI.

```
docker compose up -d                                         # backend

crewlist human add "find a reliable tree removal service"   # -> 1

crewlist agent list                                          # agent sees task 1
crewlist agent handoff 1                                     # agent gets the payload
crewlist agent add --parent 1 "Call Alex's Tree Service 617-898-0989"
crewlist agent done 1 --summary "3 vetted options"

crewlist human list                                          # the calls to make
```

## Status

Early scaffold. The specification is in [SPEC.md](SPEC.md) and is the source of
truth; the code is behind it.

| Piece | State |
|---|---|
| `crewlist-core` | Types and wire DTOs. No behavior — tests drive that in. |
| `crewlist-store` | Real. Migrations, Mongo validator, health pings. |
| `crewlist-server` | Routes wired, `/health` real, task handlers return 501. |
| `crewlist-client` | Not started. |
| `crewlist-cli` | Not started. |

## Development

```sh
docker compose up -d          # postgres, mongo, server
curl -s localhost:8787/health # {"server":{"ok":true,…},…}

cargo build                   # workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

To run the server outside Docker, copy `.env.example` and point
`CREWLIST_POSTGRES_URL` / `CREWLIST_MONGO_URL` at your own instances. The
server migrates Postgres and installs the Mongo schema validator on boot, and
refuses to listen if either fails.
