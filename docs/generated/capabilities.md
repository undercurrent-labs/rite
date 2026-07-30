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

Read a line from stdin with optional prompt.

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

Return file metadata record.

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

Remove a file or directory.

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

Format a timestamp with a pattern.

- arity: 2
- effectful: false
- permission: clock

### sleep

Sleep for a duration in milliseconds.

- arity: 1
- effectful: true
- permission: clock

### duration

Normalize a duration value to milliseconds.

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

Return allowed environment variables as a record.

- arity: 0
- effectful: true
- permission: env

## @process

### run

Run a command with argument array (no shell).

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

Build an explicit HTTP response record.

- arity: 1
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

