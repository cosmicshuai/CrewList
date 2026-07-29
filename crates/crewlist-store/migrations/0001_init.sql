-- CrewList task metadata. SPEC.md §4.
--
-- Postgres owns existence, identity, status, and hierarchy: everything you
-- filter, sort, or transition on. The payload lives in Mongo.

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

-- The agent queue: todo + human-origin + eligible + root. SPEC.md §3.3.
CREATE INDEX tasks_queue_idx  ON tasks (status, origin, agent_eligible)
                              WHERE parent_id IS NULL;
CREATE INDEX tasks_parent_idx ON tasks (parent_id);

-- The two-level depth limit is enforced in the application layer, not by a
-- trigger: the error needs a readable message and exit code 6, which a CHECK
-- constraint cannot give. See AC-31.
--
-- `updated_at` is likewise set by the application on every mutating statement
-- rather than by a trigger, so it stays testable without a live database.
