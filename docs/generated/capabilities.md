# Capabilities

## @console

### print

Write a value to stdout without a trailing newline.

- arity: 1
- effectful: true
- permission: console

### println

Write a value to stdout with a trailing newline.

- arity: 1
- effectful: true
- permission: console

### warn

Write a warning to stderr.

- arity: 1
- effectful: true
- permission: console

### error

Write an error to stderr.

- arity: 1
- effectful: true
- permission: console

### inspect

Write a debug representation to stdout.

- arity: 1
- effectful: true
- permission: console

### read_line

Read one line from stdin, after writing the optional prompt without a trailing newline. The line comes back without its terminator (`\n` or `\r\n`); end of input answers the empty string. The prompt is written by the runtime, which owns the output sink, so this is called with no argument from there.

- arity: 1
- effectful: true
- permission: console

## @fs

### read

Read a UTF-8 text file.

- arity: 1
- effectful: true
- permission: fs:read

### read_bytes

Read a file as bytes.

- arity: 1
- effectful: true
- permission: fs:read

### write

Write text to a file.

- arity: 2
- effectful: true
- permission: fs:write

### append

Append text to a file.

- arity: 2
- effectful: true
- permission: fs:write

### lines

Read file lines as a list of strings.

- arity: 1
- effectful: true
- permission: fs:read

### exists

Check whether a path exists.

- arity: 1
- effectful: true
- permission: fs:read

### metadata

Return a file metadata record: `len`, `is_file`, `is_dir`, `is_symlink`, and `mtime` as an RFC3339 UTC string (comparable against `@clock.now`). Follows symlinks, so every field but `is_symlink` describes the target.

- arity: 1
- effectful: true
- permission: fs:read

### glob

Expand a glob pattern to matching paths. The pattern must point inside a granted read root; matches outside every granted root are dropped.

- arity: 1
- effectful: true
- permission: fs:read

### mkdir

Create a directory.

- arity: 1
- effectful: true
- permission: fs:write

### remove

Remove a file, or a directory and everything inside it. Recursive and irreversible, like `rm -rf`.

- arity: 1
- effectful: true
- permission: fs:write

### copy

Copy a file.

- arity: 2
- effectful: true
- permission: fs:write

### move

Move/rename a file.

- arity: 2
- effectful: true
- permission: fs:write

## @json

### decode

Parse a JSON string into a Rite value.

- arity: 1
- effectful: false
- permission: 

### encode

Serialize a value to compact JSON.

- arity: 1
- effectful: false
- permission: 

### encode_pretty

Serialize a value to pretty JSON.

- arity: 1
- effectful: false
- permission: 

### read

Read and parse a JSON file.

- arity: 1
- effectful: true
- permission: fs:read

### write

Write a value as JSON to a file.

- arity: 2
- effectful: true
- permission: fs:write

## @csv

### decode

Parse a CSV string into a list of records (ok/err). Options: headers (default true), delimiter, skip_empty.

- arity: 1
- effectful: false
- permission: 

### encode

Serialize a list of records (or list of lists) to a CSV string.

- arity: 1
- effectful: false
- permission: 

### read

Read and parse a CSV file into a list of records.

- arity: 1
- effectful: true
- permission: fs:read

### write

Write a list of records as CSV to a file.

- arity: 2
- effectful: true
- permission: fs:write

## @crypto

### sha256

SHA-256 digest of a string, as lowercase hex.

- arity: 1
- effectful: false
- permission: 

### sha512

SHA-512 digest of a string, as lowercase hex.

- arity: 1
- effectful: false
- permission: 

### hmac_sha256

HMAC-SHA-256 of a message under a key, as lowercase hex.

- arity: 2
- effectful: false
- permission: 

### random_bytes

n cryptographically secure random bytes, as lowercase hex.

- arity: 1
- effectful: true
- permission: random

### constant_time_eq

Compare two strings in time independent of their contents.

- arity: 2
- effectful: false
- permission: 

### base64_encode

Encode a string as standard base64 (RFC 4648, padded).

- arity: 1
- effectful: false
- permission: 

### base64_decode

Decode standard base64 to a string. Answers a Result.

- arity: 1
- effectful: false
- permission: 

### hex_encode

Encode a string as lowercase hex.

- arity: 1
- effectful: false
- permission: 

### hex_decode

Decode hex to a string. Answers a Result.

- arity: 1
- effectful: false
- permission: 

## @clock

### now

Current UTC timestamp as ISO-8601 string.

- arity: 0
- effectful: true
- permission: clock

### parse

Parse an ISO-8601 timestamp.

- arity: 1
- effectful: false
- permission: clock

### format

Format an RFC3339 timestamp with a strftime pattern, e.g. `%Y-%m-%d`. Answers `ok(string)`, or `err` if the timestamp or the pattern is not valid.

- arity: 2
- effectful: false
- permission: clock

### sleep

Sleep for a duration in milliseconds.

- arity: 1
- effectful: true
- permission: clock

### duration

Normalize a duration to whole milliseconds. Accepts an integer or float of milliseconds, or a string with a unit: `250ms`, `2s`, `5m`, `1h`, `1d`. Answers `ok(int)` or `err`.

- arity: 1
- effectful: false
- permission: clock

## @env

### get

Get an environment variable or none.

- arity: 1
- effectful: true
- permission: env

### require

Get a required environment variable as result.

- arity: 1
- effectful: true
- permission: env

### all

Return the environment variables this script may read, as a record. With `--allow env` that is everything; with `--allow env=NAME,…` it is exactly the names granted. Denied when nothing is granted.

- arity: 0
- effectful: true
- permission: env

## @process

### run

Run a command with an argument list (no shell). Answers `ok(⟨status, stdout, stderr⟩)`; a non-zero exit is still `ok`, but a command that cannot be started raises. The third argument is an options record understanding `cwd` (string) and `env` (record, added to the inherited environment); any other key is an error.

- arity: 3
- effectful: true
- permission: process

### args

Arguments passed to this script after `--`, as a list of strings. Needs no permission: they are the invoker's own input to this program, not ambient state.

- arity: 0
- effectful: true
- permission: 

### which

Locate an executable on PATH. Reads the PATH environment variable and probes the filesystem, so it is effectful (`!`) and needs process *and* env access to PATH (--allow process --allow env=PATH).

- arity: 1
- effectful: true
- permission: process

## @random

### int

Random integer in [min, max].

- arity: 2
- effectful: true
- permission: random

### float

Random float in [0, 1).

- arity: 0
- effectful: true
- permission: random

### choose

Choose a random element from a list.

- arity: 1
- effectful: true
- permission: random

### shuffle

Return a shuffled copy of a list.

- arity: 1
- effectful: true
- permission: random

### uuid

Generate a UUID v4 string.

- arity: 0
- effectful: true
- permission: random

### seed

Reseed the RNG for deterministic sequences.

- arity: 1
- effectful: true
- permission: random

## @http

### listen

Start an HTTP server and block until shutdown. Loopback (127.0.0.0/8, ::1, localhost) binds by default; any other interface — including the wildcards 0.0.0.0 and [::] — needs --allow net=<host>.

- arity: 2
- effectful: true
- permission: net

### get

GET a URL. Returns ok(response) or err(record). Needs --allow net=<host>.

- arity: 1
- effectful: true
- permission: net

### post

POST a body to a URL. A record body is sent as JSON, a string as-is. Needs --allow net=<host>.

- arity: 2
- effectful: true
- permission: net

### request

Send a request described by a record: <<method, url, headers, body, timeout_ms>>. Needs --allow net=<host>.

- arity: 1
- effectful: true
- permission: net

### response

Build an explicit response record `⟨status, body⟩`. The body is optional and defaults to `none`.

- arity: 2
- effectful: false
- permission: 

### log

Middleware: log each request as `rite: METHOD path status duration` to stderr. Enable with `use @http.log` or `⊏ @http.log`.

- arity: 0
- effectful: false
- permission: 

### recover

Middleware: convert handler panics/errors into JSON 500 responses. Enable with `use @http.recover` or `⊏ @http.recover`.

- arity: 0
- effectful: false
- permission: 

## @udp

### bind

Bind a UDP socket and return ok(handle). Loopback (127.0.0.0/8, ::1, localhost) binds by default; any other interface — including the wildcards 0.0.0.0 and [::] — needs --allow net=<host>. Port 0 picks a free port; read it back with @udp.local_addr.

- arity: 1
- effectful: true
- permission: net

### local_addr

The address a socket is actually bound to, as "host:port". Returns ok(string).

- arity: 1
- effectful: true
- permission: net

### send_to

Send one datagram to "host:port". The payload is a string (sent as UTF-8) or a bytes value (sent verbatim). Returns ok(bytes sent). Needs --allow net=<host> for the destination.

- arity: 3
- effectful: true
- permission: net

### recv_from

Wait up to timeout_ms (default 1000) for one datagram. Returns ok(⟨from, data, text⟩) — `data` is bytes, `text` is the same payload as lossy UTF-8 — or err(⟨kind: "udp.timeout", …⟩) when nothing arrives. A timeout is a value, not a raise.

- arity: 2
- effectful: true
- permission: net

### close

Close a socket handle. Closing an unknown or already-closed handle answers ok(none).

- arity: 1
- effectful: true
- permission: net

## @tcp

### connect

Open a TCP connection to "host:port" and return ok(handle). Needs --allow net=<host> for the destination, including loopback. Gives up after 30 seconds with err(⟨kind: "tcp.timeout", …⟩).

- arity: 1
- effectful: true
- permission: net

### send

Write the whole payload to a connection. The payload is a string (sent as UTF-8) or a bytes value (sent verbatim). Returns ok(bytes sent).

- arity: 2
- effectful: true
- permission: net

### recv

Read up to max_bytes (default 65536), waiting at most timeout_ms (default 1000). Returns ok(bytes) — **empty** when the peer closed the stream cleanly — or err(⟨kind: "tcp.timeout", …⟩) when nothing arrived in time. Neither is a raise.

- arity: 3
- effectful: true
- permission: net

### peer_addr

The address at the other end of a connection, as "host:port". Returns ok(string). In a @tcp.listen block this is the client that connected — what a server logs.

- arity: 1
- effectful: true
- permission: net

### local_addr

This end of a connection, as "host:port". Returns ok(string). Useful on a client, where the source port is assigned rather than chosen.

- arity: 1
- effectful: true
- permission: net

### close

Close a connection handle. Closing an unknown or already-closed handle answers ok(none).

- arity: 1
- effectful: true
- permission: net

### listen

Accept TCP connections and run a block per connection: `! @tcp.listen "127.0.0.1:9000" ⟦ |conn| … ⟧`. Blocks until shutdown (Ctrl-C), like @http.listen; the connection is closed when the block returns. Loopback binds by default; any other interface — including the wildcards 0.0.0.0 and [::] — needs --allow net=<host>.

- arity: 2
- effectful: true
- permission: net

## @game

### register_item

Register an item entity.

- arity: 2
- effectful: true
- permission: 

### register_room

Register a room entity.

- arity: 2
- effectful: true
- permission: 

### register_world

Register world metadata.

- arity: 2
- effectful: true
- permission: 

### say

Emit a narrative message.

- arity: 1
- effectful: true
- permission: 

### reveal

Reveal a room or flag.

- arity: 1
- effectful: true
- permission: 

### go

Move through an exit.

- arity: 1
- effectful: true
- permission: 

### take

Add item to inventory.

- arity: 1
- effectful: true
- permission: 

### drop

Remove item from inventory.

- arity: 1
- effectful: true
- permission: 

### look

Describe current room.

- arity: 0
- effectful: false
- permission: 

### inventory

List inventory item ids.

- arity: 0
- effectful: false
- permission: 

### save

Serialize game state to JSON string.

- arity: 0
- effectful: false
- permission: 

### load

Load game state from JSON string.

- arity: 1
- effectful: true
- permission: 

### start

Start game at a room.

- arity: 1
- effectful: true
- permission: 

### command

Parse and run a player command.

- arity: 1
- effectful: true
- permission: 

### messages

Drain pending narrative messages.

- arity: 0
- effectful: false
- permission: 

### state

Return current game state record.

- arity: 0
- effectful: false
- permission: 

## @store

### get

Get a value from an in-memory namespace.

- arity: 2
- effectful: false
- permission: 

### set

Set a value in an in-memory namespace.

- arity: 3
- effectful: true
- permission: 

### delete

Delete a key from a namespace.

- arity: 2
- effectful: true
- permission: 

## @db

### open

Open a DuckDB connection. Path omitted or ":memory:" → in-memory. Needs --allow db or --allow db=path. DuckDB's own file/network access (read_csv, COPY TO, ATTACH, extensions) is sandboxed to the granted db= / fs:write roots.

- arity: 0
- effectful: true
- permission: db

### close

Close a database connection handle.

- arity: 1
- effectful: true
- permission: db

### exec

Execute SQL that does not return rows (DDL/DML). Optional params list.

- arity: 2
- effectful: true
- permission: db

### query

Run a SQL query and return ok(list of records). Optional params list.

- arity: 2
- effectful: true
- permission: db

### prepare

Prepare a SQL statement; returns a statement handle.

- arity: 2
- effectful: true
- permission: db

### query_prepared

Execute a prepared statement as a query. Optional params list.

- arity: 1
- effectful: true
- permission: db

### exec_prepared

Execute a prepared statement without returning rows. Optional params list.

- arity: 1
- effectful: true
- permission: db

### close_stmt

Drop a prepared statement handle.

- arity: 1
- effectful: true
- permission: db

### begin

BEGIN a transaction on the connection.

- arity: 1
- effectful: true
- permission: db

### commit

COMMIT the current transaction.

- arity: 1
- effectful: true
- permission: db

### rollback

ROLLBACK the current transaction.

- arity: 1
- effectful: true
- permission: db

