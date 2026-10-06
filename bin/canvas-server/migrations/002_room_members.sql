CREATE TABLE IF NOT EXISTS room_members (
    room_id TEXT NOT NULL,
    client_id TEXT NOT NULL,
    PRIMARY KEY (room_id, client_id),
    FOREIGN KEY (room_id) REFERENCES rooms(room_id) ON DELETE CASCADE
);
