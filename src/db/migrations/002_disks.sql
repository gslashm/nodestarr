-- Disk health and storage pools of the Proxmox nodes, version 2.

-- Physical disks, by a stable id that survives /dev/sdX renames and moves
-- between nodes: "<model>_<serial>", "wwn-<wwn>", or "<node>_<dev>" for disks
-- without a usable serial number.
CREATE TABLE disks (
    id          TEXT PRIMARY KEY,
    node        TEXT NOT NULL,                       -- where it was seen last
    devpath     TEXT NOT NULL,
    first_seen  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL,
    info        TEXT NOT NULL,                       -- JSON: the disk as last seen
    report      TEXT,                                -- JSON: the latest SMART report
    label       TEXT                                 -- JSON: bay, notes, purchase and warranty dates
);

-- SMART readings over time. Sleeping disks get a row too (state 'standby'),
-- so the history shows when they slept.
CREATE TABLE disk_samples (
    disk       TEXT NOT NULL,
    ts         INTEGER NOT NULL,
    state      TEXT NOT NULL,                        -- active | standby | error
    temp       REAL,
    poh        INTEGER,                              -- power-on hours
    realloc    INTEGER,
    pending    INTEGER,
    uncorrect  INTEGER,                              -- offline uncorrectable, NVMe media errors, SAS uncorrected
    crc        INTEGER,
    wear       REAL,                                 -- percent of rated endurance used
    written    INTEGER,                              -- bytes written over the disk's life
    cycles     INTEGER,                              -- head load/unload cycles
    PRIMARY KEY (disk, ts)
) WITHOUT ROWID;

-- SMART self-tests started by Nodestarr (by hand or on a schedule).
CREATE TABLE disk_tests (
    id             TEXT PRIMARY KEY,
    disk           TEXT NOT NULL,
    node           TEXT NOT NULL,
    devpath        TEXT NOT NULL,
    kind           TEXT NOT NULL,                    -- short | long | conveyance
    status         TEXT NOT NULL,                    -- queued | running | passed | failed | aborted | interrupted | skipped | error
    queued_at      INTEGER NOT NULL,
    started_at     INTEGER,
    finished_at    INTEGER,
    progress       REAL,                             -- 0..1 while running
    expected_secs  INTEGER,                          -- the drive's own estimate
    start_hours    INTEGER,                          -- power-on hours at the start
    log_top        TEXT,                             -- the drive's newest self-test log entry at the start (the result is a newer one)
    result         TEXT,                             -- the drive's wording, e.g. "Completed: read failure"
    lba            INTEGER,                          -- first failing block
    message        TEXT,                             -- why it waits, was skipped or failed to start
    actor          TEXT,
    schedule       TEXT                              -- schedule id, when scheduled
);
CREATE INDEX disk_tests_disk ON disk_tests(disk, queued_at);
CREATE INDEX disk_tests_status ON disk_tests(status);

-- What happened to a disk or pool: appeared, moved, counters changed,
-- health changed, tests, scrubs.
CREATE TABLE disk_events (
    id        INTEGER PRIMARY KEY,
    subject   TEXT NOT NULL,                         -- disk id, or "pool:<node>/<kind>/<name>"
    ts        INTEGER NOT NULL,
    severity  TEXT NOT NULL,
    kind      TEXT NOT NULL,
    message   TEXT NOT NULL
);
CREATE INDEX disk_events_subject ON disk_events(subject, ts);
CREATE INDEX disk_events_ts ON disk_events(ts);

-- Pool capacity over time (at most hourly), for growth estimates.
CREATE TABLE pool_samples (
    pool  TEXT NOT NULL,                             -- "<node>/<kind>/<name>"
    ts    INTEGER NOT NULL,
    size  INTEGER,
    used  INTEGER,
    PRIMARY KEY (pool, ts)
) WITHOUT ROWID;
