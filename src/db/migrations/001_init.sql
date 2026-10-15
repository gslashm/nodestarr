-- Nodestarr schema, version 1.
-- Timestamps are unix seconds unless the column name ends in _ms.

CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,          -- JSON document
    updated_at  INTEGER NOT NULL
);

CREATE TABLE users (
    id              INTEGER PRIMARY KEY,
    username        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash   TEXT NOT NULL,
    role            TEXT NOT NULL DEFAULT 'admin',   -- admin | viewer
    totp_secret     TEXT,                            -- base32; set during enrollment
    totp_enabled    INTEGER NOT NULL DEFAULT 0,
    recovery_codes  TEXT,                            -- JSON array of sha256 hashes
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    last_login_at   INTEGER
);

CREATE TABLE sessions (
    id            TEXT PRIMARY KEY,                  -- sha256 of the cookie token
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL,
    last_seen_at  INTEGER NOT NULL,
    ip            TEXT,
    user_agent    TEXT,
    mfa_pending   INTEGER NOT NULL DEFAULT 0         -- password accepted, TOTP outstanding
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE INDEX sessions_expires ON sessions(expires_at);

CREATE TABLE api_keys (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    key_hash      TEXT NOT NULL UNIQUE,              -- sha256 of the key
    prefix        TEXT NOT NULL,                     -- first characters, for display
    role          TEXT NOT NULL DEFAULT 'admin',
    created_at    INTEGER NOT NULL,
    last_used_at  INTEGER
);

-- Per-container preferences keyed by a stable identity that survives
-- recreation: "compose:<project>/<service>" or "container:<name>".
CREATE TABLE container_prefs (
    identity    TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE activity (
    id           INTEGER PRIMARY KEY,
    ts           INTEGER NOT NULL,
    kind         TEXT NOT NULL,                      -- e.g. update.applied
    severity     TEXT NOT NULL,                      -- info | success | warning | error
    target       TEXT,                               -- identity or other key
    target_name  TEXT,
    message      TEXT NOT NULL,
    actor        TEXT,
    details      TEXT                                -- JSON
);
CREATE INDEX activity_ts ON activity(ts);
CREATE INDEX activity_target ON activity(target, ts);
CREATE INDEX activity_kind ON activity(kind, ts);

CREATE TABLE tasks (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    title        TEXT NOT NULL,
    target       TEXT,                               -- identity of the container (or other key)
    target_name  TEXT,
    status       TEXT NOT NULL,                      -- succeeded | failed | cancelled
    actor        TEXT,
    created_at   INTEGER NOT NULL,
    started_at   INTEGER,
    finished_at  INTEGER,
    log          TEXT,
    result       TEXT,                               -- JSON
    error        TEXT
);
CREATE INDEX tasks_created ON tasks(created_at);

-- Last known update-check state per container, so badges survive restarts
-- and "first seen" times for new digests can enforce a minimum age.
CREATE TABLE update_state (
    identity    TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE update_history (
    id             INTEGER PRIMARY KEY,
    identity       TEXT NOT NULL,
    target_name    TEXT,
    ts             INTEGER NOT NULL,
    from_ref       TEXT,
    to_ref         TEXT,
    from_image_id  TEXT,
    to_image_id    TEXT,
    from_version   TEXT,
    to_version     TEXT,
    rollback_tag   TEXT,                             -- local tag that pins the old image
    compose_file   TEXT,                             -- path edited for a tag change
    compose_before TEXT,                             -- previous compose content (tag changes)
    status         TEXT NOT NULL,                    -- applied | failed | rolled_back
    trigger        TEXT,                             -- manual | auto | api | mqtt
    task_id        TEXT,
    message        TEXT
);
CREATE INDEX update_history_identity ON update_history(identity, ts);
CREATE INDEX update_history_ts ON update_history(ts);

CREATE TABLE backups (
    id           TEXT PRIMARY KEY,
    identity     TEXT NOT NULL,
    target_name  TEXT,
    path         TEXT NOT NULL,                      -- relative to the backup root
    size         INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    kind         TEXT NOT NULL,                      -- manual | scheduled | pre-update | pre-restore
    status       TEXT NOT NULL,                      -- ok | partial
    manifest     TEXT NOT NULL,                      -- JSON
    pinned       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX backups_identity ON backups(identity, created_at);

-- Container resource samples. res is the bucket size in seconds: raw
-- samples use the sampling interval, rollups use 900 (15 minutes).
CREATE TABLE metrics (
    identity  TEXT NOT NULL,
    res       INTEGER NOT NULL,
    ts        INTEGER NOT NULL,
    cpu       REAL,                                  -- percent of one core x cores
    mem       INTEGER,                               -- bytes
    mem_limit INTEGER,
    net_rx    INTEGER,                               -- bytes/s
    net_tx    INTEGER,
    blk_r     INTEGER,                               -- bytes/s
    blk_w     INTEGER,
    PRIMARY KEY (identity, res, ts)
) WITHOUT ROWID;

CREATE TABLE uptime_results (
    check_id    TEXT NOT NULL,
    ts          INTEGER NOT NULL,
    ok          INTEGER NOT NULL,
    latency_ms  INTEGER,
    status      INTEGER,
    error       TEXT,
    PRIMARY KEY (check_id, ts)
) WITHOUT ROWID;

CREATE TABLE uptime_hourly (
    check_id    TEXT NOT NULL,
    hour        INTEGER NOT NULL,                    -- unix seconds, hour aligned
    ok_count    INTEGER NOT NULL,
    fail_count  INTEGER NOT NULL,
    latency_sum INTEGER NOT NULL,
    PRIMARY KEY (check_id, hour)
) WITHOUT ROWID;

CREATE TABLE alerts (
    id           INTEGER PRIMARY KEY,
    rule_id      TEXT NOT NULL,
    target       TEXT NOT NULL DEFAULT '',
    target_name  TEXT,
    severity     TEXT NOT NULL,
    message      TEXT NOT NULL,
    started_at   INTEGER NOT NULL,
    resolved_at  INTEGER,
    details      TEXT
);
CREATE INDEX alerts_open ON alerts(resolved_at, rule_id, target);
CREATE INDEX alerts_started ON alerts(started_at);

-- Previous versions of files edited through Nodestarr (compose files and app
-- config files), zstd compressed.
CREATE TABLE file_versions (
    id        INTEGER PRIMARY KEY,
    path      TEXT NOT NULL,
    ts        INTEGER NOT NULL,
    actor     TEXT,
    size      INTEGER NOT NULL,
    content   BLOB NOT NULL,
    note      TEXT
);
CREATE INDEX file_versions_path ON file_versions(path, ts);
