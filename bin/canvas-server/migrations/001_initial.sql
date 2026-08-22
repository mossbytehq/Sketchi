CREATE TABLE IF NOT EXISTS rooms (
    room_id TEXT PRIMARY KEY NOT NULL,
    token_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS room_creators (
    room_id TEXT PRIMARY KEY NOT NULL,
    creator_id TEXT NOT NULL,
    FOREIGN KEY (room_id) REFERENCES rooms(room_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS room_creator_tokens (
    room_id TEXT PRIMARY KEY NOT NULL,
    token_hash TEXT NOT NULL,
    FOREIGN KEY (room_id) REFERENCES rooms(room_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS operations (
    room_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (room_id, operation_id),
    FOREIGN KEY (room_id) REFERENCES rooms(room_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS snapshots (
    room_id TEXT PRIMARY KEY NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (room_id) REFERENCES rooms(room_id) ON DELETE CASCADE
);
