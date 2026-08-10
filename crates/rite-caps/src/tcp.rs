//! `@tcp` capability: connection-oriented byte streams (native only).
//!
//! `@udp` is the connectionless half of the network host; this is the other one. A
//! connection is an opaque `Value::Handle`, the representation `@db` and `@udp`
//! already use, and the client half reads the same way a datagram socket does:
//! `connect` → `send`/`recv` → `close`, with `close` on an already-closed handle a
//! no-op because a script closes on the way out.
//!
//! **The server is callback-shaped, not an accept loop.**
//!
//! ```text
//! ! @tcp.listen "127.0.0.1:9000" ⟦ |conn|
//!   msg ← ! @tcp.recv(conn, 1024, 5000)?
//!   ! @tcp.send(conn, msg)?
//! ⟧
//! ```
//!
//! The block runs once per accepted connection, in its own task, and the connection
//! is closed when it returns — exactly the lifetime `@http.listen` gives a request
//! handler, and `listen` blocks until shutdown for the same reason. Exposing
//! `accept` instead would hand the script a connection with no defined lifetime, and
//! the language has no answer for that: no destructors, no scope-bound resources.
//! One shape the language can already express beats two it cannot.
//!
//! **Bytes.** Payloads are the `bytes` type, as in `@udp`: `send` takes a string
//! (sent as its UTF-8 bytes) or a bytes value (sent verbatim), and `recv` answers
//! bytes. They are built and read with the byte builtins — `from_hex`, `bytes`,
//! `to_hex`, `to_text`, `concat`, `slice`, `byte_at` — so nothing here invents a
//! `@tcp`-local spelling of binary data.
//!
//! **A stream ends, and waiting is ordinary.** `recv` distinguishes the two:
//!
//! * The peer closed cleanly → `ok` with **zero bytes**. That is the end of the
//!   stream, and it is a value, so a read loop terminates by testing the length.
//! * Nothing arrived in time → `err(⟨kind: "tcp.timeout", …⟩)`. The connection is
//!   still open; the script decides whether to wait again or give up.
//!
//! Neither is a raise, for the same reason `@udp.recv_from`'s timeout is not.
//!
//! Disabled on wasm targets — calls return a clear capability error, as `@process`
//! and `@udp` do.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, RuntimeContext, Value};

#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use rite_runtime::{Evaluator, HostHandle, Key};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{oneshot, Mutex as AsyncMutex};

/// Handle kind, so a `@udp` socket handed to `@tcp.close` is caught by name.
#[cfg(not(target_arch = "wasm32"))]
const HANDLE_KIND: &str = "tcp.conn";

/// How much `recv` reads when the call does not say. One read returns *up to* this
/// many bytes — TCP is a stream, so a short read is normal and not an error.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_MAX_BYTES: i64 = 65_536;

/// Ceiling on `max_bytes`, so a typo cannot ask for a gigabyte-sized buffer.
#[cfg(not(target_arch = "wasm32"))]
const MAX_RECV_BYTES: i64 = 16 * 1024 * 1024;

/// How long `recv` waits when the call does not say — the `@udp.recv_from` default.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_TIMEOUT_MS: i64 = 1_000;

/// How long `connect` waits for the handshake. The same ceiling `@http` puts on an
/// outbound request: a filtered host would otherwise hang the script indefinitely.
#[cfg(not(target_arch = "wasm32"))]
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// One live connection, its halves locked separately.
///
/// A single lock over the whole stream would make a blocking `recv` stop a `send` on
/// the same connection, which is precisely what a full-duplex protocol needs to do.
/// `into_split` gives two independently owned halves; dropping both closes the
/// socket, which is what `close` does.
#[cfg(not(target_arch = "wasm32"))]
struct Conn {
    read: Arc<AsyncMutex<OwnedReadHalf>>,
    write: Arc<AsyncMutex<OwnedWriteHalf>>,
    /// Both addresses are read once, here, and then never again.
    ///
    /// They are fixed for the life of a TCP connection, so there is nothing to
    /// re-query — and querying later would have to go through a half's mutex, which
    /// a blocked `recv` is holding. That would make `@tcp.peer_addr` hang precisely
    /// when a server most wants it: logging who is on the other end of a connection
    /// that has gone quiet. `None` only if the socket could not answer at accept
    /// time, which is a connection already dead.
    peer: Option<String>,
    local: Option<String>,
}

/// Register a connection on the context that will use it.
///
/// This was a process-global map, on the reasoning that a `@tcp.listen` handler
/// runs its block in a *fresh* `RuntimeContext` — so a table hanging off the
/// listening capability would be invisible to the very block the handle is passed
/// to. That much is true, and it is why the table cannot live on the capability.
/// It does not follow that it has to be global: the handler's own context is
/// reachable at the point the connection is registered, and it is the right owner.
///
/// Being global cost a real property. A socket outlived the run that opened it,
/// so `rite run` only cleaned up because the process exited — and inside
/// `RiteEngine`, where the host process keeps going, a guest that never called
/// `@tcp.close` leaked the connection for the lifetime of the host, with no way
/// for the next run to reach it. The same reasoning `@fs.open` follows.
#[cfg(not(target_arch = "wasm32"))]
fn register(stream: TcpStream, ctx: &RuntimeContext) -> Result<u64, EvalError> {
    // Before the split: both halves can answer, but only while nothing holds them.
    let peer = stream.peer_addr().ok().map(|a| a.to_string());
    let local = stream.local_addr().ok().map(|a| a.to_string());
    let (r, w) = stream.into_split();
    ctx.handles
        .insert(
            HANDLE_KIND,
            Box::new(Conn {
                read: Arc::new(AsyncMutex::new(r)),
                write: Arc::new(AsyncMutex::new(w)),
                peer,
                local,
            }),
        )
        .map_err(|limit| {
            EvalError::Message(format!(
                "tcp: too many open connections ({limit}). A connection is closed with \
                 @tcp.close, or when the run ends"
            ))
        })
}

/// Most recent address `@tcp.listen` actually bound.
///
/// `listen` blocks until shutdown, so a test that asks for port 0 has no way to read
/// the port back out of the call. This is the same side channel, and for the same
/// reason, as `@http`'s `last_bound_addr`: observation only — nothing in the accept
/// path reads it.
#[cfg(not(target_arch = "wasm32"))]
static LAST_BOUND_ADDR: Mutex<Option<String>> = Mutex::new(None);

/// Address of the last successful `@tcp.listen` bind (e.g. `"127.0.0.1:54321"`).
#[cfg(not(target_arch = "wasm32"))]
pub fn last_bound_addr() -> Option<String> {
    LAST_BOUND_ADDR.lock().clone()
}

/// Clear the last-bound address (tests), so a stale value from an earlier server
/// cannot be mistaken for the one just started.
#[cfg(not(target_arch = "wasm32"))]
pub fn clear_last_bound_addr() {
    *LAST_BOUND_ADDR.lock() = None;
}

#[derive(Default)]
pub struct TcpCap;

impl TcpCap {
    pub fn new() -> Self {
        Self
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "connect",
            docs: "Open a TCP connection to \"host:port\" and return ok(handle). Needs --allow net=<host> for the destination, including loopback. Gives up after 30 seconds with err(⟨kind: \"tcp.timeout\", …⟩).",
            arity: 1,
            effectful: true,
            permission: "net",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "send",
            docs: "Write the whole payload to a connection. The payload is a string (sent as UTF-8) or a bytes value (sent verbatim). Returns ok(bytes sent).",
            arity: 2,
            effectful: true,
            permission: "net",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "recv",
            docs: "Read up to max_bytes (default 65536), waiting at most timeout_ms (default 1000). Returns ok(bytes) — **empty** when the peer closed the stream cleanly — or err(⟨kind: \"tcp.timeout\", …⟩) when nothing arrived in time. Neither is a raise.",
            arity: 3,
            effectful: true,
            permission: "net",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "peer_addr",
            docs: "The address at the other end of a connection, as \"host:port\". Returns ok(string). In a @tcp.listen block this is the client that connected — what a server logs.",
            arity: 1,
            effectful: true,
            permission: "net",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "local_addr",
            docs: "This end of a connection, as \"host:port\". Returns ok(string). Useful on a client, where the source port is assigned rather than chosen.",
            arity: 1,
            effectful: true,
            permission: "net",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "close",
            docs: "Close a connection handle. Closing an unknown or already-closed handle answers ok(none).",
            arity: 1,
            effectful: true,
            permission: "net",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "listen",
            docs: "Accept TCP connections and run a block per connection: `! @tcp.listen \"127.0.0.1:9000\" ⟦ |conn| … ⟧`. Blocks until shutdown (Ctrl-C), like @http.listen; the connection is closed when the block returns. Loopback binds by default; any other interface — including the wildcards 0.0.0.0 and [::] — needs --allow net=<host>.",
            arity: 2,
            effectful: true,
            permission: "net",
            returns_result: false,
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (method, args, perms, ctx);
            Err(EvalError::Capability(
                "@tcp requires the native host: the browser runtime has no socket layer".into(),
            ))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            match method {
                "connect" => self.connect(args, perms, ctx).await,
                "send" => self.send(args, ctx).await,
                "recv" => self.recv(args, ctx).await,
                "peer_addr" => conn_addr(args.first(), Endpoint::Peer, ctx),
                "local_addr" => conn_addr(args.first(), Endpoint::Local, ctx),
                "close" => close(args.first(), ctx),
                "listen" => self.listen(args, perms, ctx).await,
                other => Err(EvalError::Capability(format!("unknown @tcp.{}", other))),
            }
        }
    }

    /// Dial out. The destination is gated per host, exactly as an outbound `@http`
    /// request and a `@udp.send_to` destination are — **including loopback**, which
    /// is not exempt the way a *bind* address is: connecting somewhere is reaching
    /// out, and `127.0.0.1` is where the interesting local services live.
    #[cfg(not(target_arch = "wasm32"))]
    async fn connect(
        &self,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        let addr = args
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                EvalError::Message("tcp.connect expects an address \"host:port\"".into())
            })?
            .to_string();
        // Matched as written; a name is resolved afterwards, so `--allow net=example.com`
        // does not become a grant for wherever DNS points that name next.
        let host = crate::http::addr_host(&addr);
        perms.check_net(&host).map_err(EvalError::Permission)?;

        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr)).await {
            Ok(Ok(stream)) => Ok(Value::ok(Value::Handle(HostHandle {
                kind: HANDLE_KIND.into(),
                id: register(stream, ctx)?,
            }))),
            // A refused connection is a condition a script can handle, so it is a
            // value — the same choice `@fs` makes for io errors.
            Ok(Err(e)) => Ok(tcp_err("tcp.connect", &addr, &e.to_string())),
            Err(_elapsed) => {
                let ms = CONNECT_TIMEOUT.as_millis() as i64;
                Ok(timeout_err(
                    "tcp.connect",
                    ms,
                    &format!("no connection to {addr} within {ms}ms"),
                ))
            }
        }
    }

    /// Write the *whole* payload. A partial write is not reported as success: TCP is
    /// a stream, and "I sent 3 of your 9 bytes" is a bug waiting to be a protocol
    /// error, so `write_all` loops and the answer is the payload length or an error.
    #[cfg(not(target_arch = "wasm32"))]
    async fn send(&self, args: Vec<Value>, ctx: &RuntimeContext) -> Result<Value, EvalError> {
        let write = conn_write(args.first(), ctx)?;
        let payload = payload_bytes(args.get(1))?;
        let mut half = write.lock().await;
        match half.write_all(&payload).await {
            Ok(()) => Ok(Value::ok(Value::Int(payload.len() as i64))),
            Err(e) => Ok(tcp_err("tcp.send", "", &e.to_string())),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn recv(&self, args: Vec<Value>, ctx: &RuntimeContext) -> Result<Value, EvalError> {
        let read = conn_read(args.first(), ctx)?;
        let max = int_arg(
            args.get(1),
            DEFAULT_MAX_BYTES,
            "tcp.recv expects max_bytes as an integer",
        )?;
        // Zero is refused rather than clamped: a read of an empty buffer completes
        // immediately with zero bytes, which is the signal reserved for the peer
        // closing the stream. Answering "the connection ended" because the caller
        // asked for nothing would be the worst kind of wrong.
        if max < 1 {
            return Err(EvalError::Message(
                "tcp.recv expects max_bytes of at least 1: a zero-byte read cannot be \
                 told apart from the peer closing the stream"
                    .into(),
            ));
        }
        let max = max.min(MAX_RECV_BYTES) as usize;
        let ms = int_arg(
            args.get(2),
            DEFAULT_TIMEOUT_MS,
            "tcp.recv expects timeout_ms as an integer",
        )?
        .max(0) as u64;

        let mut buf = vec![0u8; max];
        let mut half = read.lock().await;
        let got =
            tokio::time::timeout(std::time::Duration::from_millis(ms), half.read(&mut buf)).await;

        match got {
            // Nothing arrived in time. The connection is still open — this says
            // "not yet", which is why it is an `err` value and not a raise.
            Err(_elapsed) => Ok(timeout_err(
                "tcp.recv",
                ms as i64,
                &format!("no data within {ms}ms"),
            )),
            Ok(Err(e)) => Ok(tcp_err("tcp.recv", "", &e.to_string())),
            // Zero bytes from a *completed* read is end-of-stream: the peer closed
            // cleanly. That is an answer, not a failure, and it is deliberately a
            // different shape from the timeout above — `len(data) = 0` ends a read
            // loop, `err` retries it.
            Ok(Ok(n)) => {
                buf.truncate(n);
                Ok(Value::ok(Value::Bytes(buf.into())))
            }
        }
    }

    /// Serve, blocking until shutdown, with the handler block run per connection.
    #[cfg(not(target_arch = "wasm32"))]
    async fn listen(
        &self,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        let addr = args
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:0")
            .to_string();
        let handler = match args.get(1) {
            Some(Value::Function(c)) => c.clone(),
            _ => {
                return Err(EvalError::Message(
                    "tcp.listen expects a handler block: `! @tcp.listen \"127.0.0.1:9000\" ⟦ |conn| … ⟧`"
                        .into(),
                ))
            }
        };
        // The same policy `@http.listen` applies, through the same function.
        crate::http::check_bind_perm(&addr, perms, "listen")?;

        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| EvalError::Capability(format!("bind failed: {}", e)))?;
        let local = listener
            .local_addr()
            .map_err(|e| EvalError::Capability(e.to_string()))?
            .to_string();
        *LAST_BOUND_ADDR.lock() = Some(local.clone());
        // Port 0 is useless without this, which is why `@http.listen` prints it too.
        println!("rite: listening on tcp://{}", local);
        let _ = std::io::Write::flush(&mut std::io::stdout());

        // Ctrl-C stops the loop, as it does for `@http.listen` — through the same
        // stop switch, which a handler calling `@process.exit` also reaches.
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let stop = crate::http::arm_server_stop(shutdown_tx);
        let _ = ctrlc::set_handler(crate::http::request_stop);

        // Everything a handler needs to resolve the names it was written next to,
        // captured once at listen time. Cloning an `Environment` shares its frames,
        // so this is a handful of `Arc` bumps per connection, and module state
        // persists across connections — a top-level counter counts every one.
        // The capability host and handle table are the listen-time context's own,
        // shared exactly as `@http.listen` shares them: a `@db` connection opened
        // before `listen` works inside a connection handler.
        let scope = ConnScope {
            module_env: ctx.env.clone(),
            functions: ctx.functions.clone(),
            capabilities: ctx.capabilities.clone(),
            handles: ctx.handles.clone(),
            sources: ctx.sources.clone(),
            budget: ctx.budget.clone(),
        };

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                acc = listener.accept() => {
                    let Ok((stream, _peer)) = acc else { break };
                    // One task per connection, exactly as `@http` spawns one per
                    // accepted socket: a slow handler must not stop the next accept.
                    let handler = handler.clone();
                    let perms = perms.clone();
                    let scope = scope.clone();
                    tokio::spawn(async move {
                        serve_conn(stream, handler, perms, scope).await;
                    });
                }
            }
        }

        stop.release();

        // As in `@http.listen`: a connection handler that exited chose a status for
        // the whole script, so `listen` fails with it rather than returning stopped.
        if let Some(code) = crate::http::take_exit_request() {
            return Err(EvalError::Exit(code));
        }

        Ok(Value::record(vec![
            (Key::String("addr".into()), Value::string(local)),
            (Key::String("status".into()), Value::string("stopped")),
        ]))
    }
}

/// The listen-time state every connection handler runs against — the
/// `@tcp` twin of `@http`'s `ServerState`.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct ConnScope {
    module_env: rite_runtime::Environment,
    functions: HashMap<String, rite_runtime::FunctionEntry>,
    capabilities: std::sync::Arc<dyn rite_runtime::CapabilityHost>,
    handles: std::sync::Arc<rite_runtime::HandleTable>,
    sources: rite_core::SourceMap,
    budget: rite_runtime::ExecutionBudget,
}

/// Run one connection's handler block, then close the connection.
///
/// The block gets its own `RuntimeContext` — own console buffers, own budget
/// counters — over the server's shared capability host, handle table and
/// module scope, so it resolves top-level bindings and functions and can use
/// host state opened before `listen`.
#[cfg(not(target_arch = "wasm32"))]
async fn serve_conn(
    stream: TcpStream,
    handler: rite_runtime::Closure,
    perms: PermissionSet,
    scope: ConnScope,
) {
    let mut ctx = RuntimeContext::new();
    ctx.capabilities = scope.capabilities;
    ctx.handles = scope.handles;
    ctx.allow_all = perms.allow_all;
    ctx.console_allowed = perms.allow_all || perms.console;
    ctx.sources = scope.sources;
    crate::http::install_module_scope(&mut ctx, &scope.module_env, &scope.functions);
    // Registered on the *handler's* context, so the connection is reachable from
    // the block it is passed to and is dropped, closing the socket, when that
    // block's context goes, which is what the explicit `unregister` below used to
    // do by hand.
    let id = match register(stream, &ctx) {
        Ok(id) => id,
        Err(e) => {
            crate::http::emit_process_stderr(&format!("rite: tcp accept: {e}\n"));
            return;
        }
    };
    // A connection is not a request. A wall clock would kill an open-but-idle
    // session on its next step, so it is lifted while the listen-time step,
    // depth and size ceilings stay in place with fresh counters.
    let mut budget = scope.budget;
    budget.restart();
    budget.timeout = None;
    ctx.budget = budget;

    let conn = Value::Handle(HostHandle {
        kind: HANDLE_KIND.into(),
        id,
    });
    let result = {
        let mut eval = Evaluator::new(&mut ctx);
        eval.call_value_public(Value::Function(handler), vec![conn])
            .await
    };
    // Handler console output is buffered on the per-connection context, as it is per
    // request in `@http` — flush it so `! @console.println` reaches the server process.
    crate::http::flush_handler_io(&ctx);
    if let Err(e) = result {
        match e {
            // Not `EvalError::Return`: `^ value` is how a handler ends early.
            EvalError::Return(_) => {}
            // An exit is a decision about the process, not a handler error to log:
            // record the status and stop accepting.
            EvalError::Exit(code) => crate::http::request_exit(code),
            e => crate::http::emit_process_stderr(&format!("rite: tcp handler error: {e}\n")),
        }
    }
    // The connection's lifetime is the block's. This close is load-bearing:
    // the handle table is shared with the listen-time context now, so dropping
    // this handler's `ctx` no longer drops the socket.
    ctx.handles.close(id);
}

/// A payload is text or bytes. Anything else is rejected rather than stringified:
/// silently sending `<bytes len=3>` down the wire is the wrong failure.
#[cfg(not(target_arch = "wasm32"))]
fn payload_bytes(v: Option<&Value>) -> Result<Vec<u8>, EvalError> {
    match v {
        Some(Value::String(s)) => Ok(s.as_bytes().to_vec()),
        Some(Value::Bytes(b)) => Ok(b.to_vec()),
        Some(other) => Err(EvalError::Message(format!(
            "tcp.send expects a string or bytes payload, got {}",
            other.type_name()
        ))),
        None => Err(EvalError::Message(
            "tcp.send expects a string or bytes payload".into(),
        )),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn int_arg(v: Option<&Value>, default: i64, message: &str) -> Result<i64, EvalError> {
    match v {
        None | Some(Value::None) => Ok(default),
        Some(v) => v
            .as_int()
            .ok_or_else(|| EvalError::Message(message.to_string())),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle_id(v: Option<&Value>) -> Result<u64, EvalError> {
    match v {
        Some(Value::Handle(h)) if h.kind == HANDLE_KIND => Ok(h.id),
        Some(Value::Handle(h)) => Err(EvalError::Message(format!(
            "expected handle {}, got {}",
            HANDLE_KIND, h.kind
        ))),
        _ => Err(EvalError::Message(format!(
            "expected handle {} from @tcp.connect or a @tcp.listen block",
            HANDLE_KIND
        ))),
    }
}

/// Clone a half out from under the registry lock, so an awaited read never holds it
/// (which would serialize every other connection, and trip
/// `clippy::await_holding_lock`).
#[cfg(not(target_arch = "wasm32"))]
fn conn_read(
    v: Option<&Value>,
    ctx: &RuntimeContext,
) -> Result<Arc<AsyncMutex<OwnedReadHalf>>, EvalError> {
    let id = handle_id(v)?;
    ctx.handles
        .with::<Conn, _>(id, |c| c.read.clone())
        .ok_or_else(|| EvalError::Message("tcp connection closed or invalid".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn conn_write(
    v: Option<&Value>,
    ctx: &RuntimeContext,
) -> Result<Arc<AsyncMutex<OwnedWriteHalf>>, EvalError> {
    let id = handle_id(v)?;
    ctx.handles
        .with::<Conn, _>(id, |c| c.write.clone())
        .ok_or_else(|| EvalError::Message("tcp connection closed or invalid".into()))
}

/// Which end of the connection an address query is asking about.
#[cfg(not(target_arch = "wasm32"))]
enum Endpoint {
    Peer,
    Local,
}

/// `@tcp.peer_addr` / `@tcp.local_addr`.
///
/// Reads the address captured when the connection was registered — see `Conn`. A
/// closed or unknown handle is a plain error rather than an `err` value, matching
/// `send` and `recv`: using a handle after closing it is a bug in the script, not a
/// condition the network produced.
#[cfg(not(target_arch = "wasm32"))]
fn conn_addr(v: Option<&Value>, which: Endpoint, ctx: &RuntimeContext) -> Result<Value, EvalError> {
    let id = handle_id(v)?;
    let addr = ctx
        .handles
        .with::<Conn, _>(id, |c| match which {
            Endpoint::Peer => c.peer.clone(),
            Endpoint::Local => c.local.clone(),
        })
        .ok_or_else(|| EvalError::Message("tcp connection closed or invalid".into()))?;
    let op = match which {
        Endpoint::Peer => "tcp.peer_addr",
        Endpoint::Local => "tcp.local_addr",
    };
    Ok(match addr {
        Some(a) => Value::ok(Value::string(a)),
        None => tcp_err(op, "", "the socket could not report its address"),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn close(v: Option<&Value>, ctx: &RuntimeContext) -> Result<Value, EvalError> {
    let id = handle_id(v)?;
    // Dropping both halves closes the socket. An unknown id is nothing to do —
    // closing twice stays fine, which is the convention this capability set.
    ctx.handles.close(id);
    Ok(Value::ok(Value::None))
}

/// Transport failures are values, in the shape `@udp` and `@http` use.
#[cfg(not(target_arch = "wasm32"))]
fn tcp_err(op: &str, addr: &str, message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("tcp.error")),
        (Key::String("operation".into()), Value::string(op)),
        (Key::String("address".into()), Value::string(addr)),
        (Key::String("message".into()), Value::string(message)),
    ]))
}

/// "Not yet" — a different kind from `tcp.error`, so `e.kind` tells them apart.
#[cfg(not(target_arch = "wasm32"))]
fn timeout_err(op: &str, timeout_ms: i64, message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("tcp.timeout")),
        (Key::String("operation".into()), Value::string(op)),
        (Key::String("message".into()), Value::string(message)),
        (Key::String("timeout_ms".into()), Value::Int(timeout_ms)),
    ]))
}
