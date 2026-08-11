# Collaboration protocol

The initial wire format is versioned JSON. Every envelope contains
`protocol_version: 1` and a tagged message kind. The protocol crate reuses
`canvas-core` identifiers, operations, snapshots, and version vectors; it does
not define a parallel mutation language. Version vectors and snapshots use
sorted arrays in JSON so UUID-based IDs never become non-portable object keys.

The session flow is:

```text
Hello -> Welcome -> JoinRoom -> Snapshot + Operations
SubmitOperations -> Ack + Operations
Presence <-----------------> Presence
Ping -> Pong
```

The client message set includes `Hello`, `CreateRoom`, `JoinRoom`,
`SubmitOperations`, `RequestSync`, `Presence`, `Ping`, `LeaveRoom`, and the
ephemeral `StrokeStart`/`StrokeChunk`/`StrokeEnd` messages. Server responses
include `Welcome`, `RoomCreated`, `Snapshot`, `Operations`, `Ack`, presence and
user lifecycle events, `SyncComplete`, `Pong`, structured `Error`, and the
ephemeral stroke echoes.

Durable operation submission is acknowledged only after the server has
validated, applied, and committed it. Acknowledgements are retry-safe because
operation IDs are idempotent. A reconnect requests a snapshot plus the delta
after the client's version vector.

Presence contains client ID, cursor, selection, and active tool. It is
throttled, bounded, and never written to the operation log or snapshots. Live
freehand previews use `StrokeStart`, `StrokeChunk`, and `StrokeEnd`; only the
final freehand operation is durable.

Dropped PNG and JPEG images are durable `Create` operations. Their original
encoded bytes, MIME type, and decoded dimensions are embedded in the image
element, so the same operation and later snapshots reproduce the image for
every collaborator. Each image is limited to 4 MiB of source bytes, 8,192
pixels per dimension, and 16 megapixels decoded; unsupported or malformed
image payloads are rejected by `canvas-core` before acknowledgement.

Message and field limits are enforced before allocation-heavy work. Unknown
protocol versions, invalid capability tokens, malformed IDs, oversized text or
point lists, duplicate operation IDs in a batch, and invalid operations receive
structured errors. JSON frames are capped at 16 MiB, durable operation batches
at 256 operations, presence selections at 256 IDs, and live stroke chunks at
2,048 points.
