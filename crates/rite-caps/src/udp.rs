//! `@udp` capability: connectionless datagram sockets (native only).
//!
//! Sockets are opaque `Value::Handle`s, the representation `@db` already uses for a
//! long-lived host resource. A datagram socket has no connection state, so the whole
//! lifetime is `bind` → `send_to`/`recv_from` → `close`; dropping the last reference
//! closes the socket, and `close` on an already-closed handle is not an error.
//!
//! **Bytes.** Rite strings are UTF-8, so a payload that is not valid UTF-8 needs a
//! separate representation. This capability uses `Value::Bytes` — the same type
//! `@fs.read_bytes` returns and `@http` puts in a response `body` — rather than
//! inventing a hex spelling of its own:
//!
//! * `send_to` accepts a **string** (sent as its UTF-8 bytes) or a **bytes** value
//!   (sent verbatim). Nothing else, so a record or an int is a mistake, not a
//!   surprise encoding.
//! * `recv_from` answers `data` as bytes, plus `text` — the same datagram decoded as
//!   UTF-8 with invalid sequences replaced, which is what `@http` does for a response
//!   body.
//!
//! `Value::Bytes` is opaque in Rite today: `len` works and two byte values compare,
//! but there is no builtin that *builds* bytes from anything but a string, and none
//! that renders them as hex. A program can therefore echo bytes it received, or send
//! text, but it cannot author a binary packet (a DNS query, say) from source. That is
//! a gap in the language's byte support, recorded in `IMPLEMENTATION.md`; the fix is a
//! general pair of conversions, not a `@udp`-local escape hatch.
//!
//! Disabled on wasm targets — calls return a clear capability error, as `@db` does.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Value};

#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use rite_runtime::{HostHandle, Key};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::UdpSocket;

/// Handle kind, so a `@db` connection passed to `@udp.close` is caught by name.
#[cfg(not(target_arch = "wasm32"))]
const HANDLE_KIND: &str = "udp.socket";

/// The largest a UDP payload can be: 65535 minus the 8-byte header and the 20-byte
/// IPv4 header. Rounded up to the theoretical maximum so nothing is truncated.
#[cfg(not(target_arch = "wasm32"))]
const MAX_DATAGRAM: usize = 65_535;

/// How long `recv_from` waits when the call does not say.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_TIMEOUT_MS: i64 = 1_000;

#[cfg(not(target_arch = "wasm32"))]
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub struct UdpCap {
    /// `id → socket`. Held behind a mutex rather than an `RwLock` on the whole
    /// capability so a blocking `recv_from` cannot stop another socket sending: the
    /// lock is only held long enough to clone the `Arc` out.
    #[cfg(not(target_arch = "wasm32"))]
    sockets: Mutex<HashMap<u64, Arc<UdpSocket>>>,
}

impl UdpCap {
    pub fn new() -> Self {
        Self::default()
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "bind",
            docs: "Bind a UDP socket and return ok(handle). Loopback (127.0.0.0/8, ::1, localhost) binds by default; any other interface — including the wildcards 0.0.0.0 and [::] — needs --allow net=<host>. Port 0 picks a free port; read it back with @udp.local_addr.",
            arity: 1,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "local_addr",
            docs: "The address a socket is actually bound to, as \"host:port\". Returns ok(string).",
            arity: 1,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "send_to",
            docs: "Send one datagram to \"host:port\". The payload is a string (sent as UTF-8) or a bytes value (sent verbatim). Returns ok(bytes sent). Needs --allow net=<host> for the destination.",
            arity: 3,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "recv_from",
            docs: "Wait up to timeout_ms (default 1000) for one datagram. Returns ok(⟨from, data, text⟩) — `data` is bytes, `text` is the same payload as lossy UTF-8 — or err(⟨kind: \"udp.timeout\", …⟩) when nothing arrives. A timeout is a value, not a raise.",
            arity: 2,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "close",
            docs: "Close a socket handle. Closing an unknown or already-closed handle answers ok(none).",
            arity: 1,
            effectful: true,
            permission: "net",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (method, args, perms);
            Err(EvalError::Capability(
                "@udp requires the native host: the browser runtime has no socket layer".into(),
            ))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            match method {
                "bind" => self.bind(args, perms).await,
                "local_addr" => self.local_addr(args),
                "send_to" => self.send_to(args, perms).await,
                "recv_from" => self.recv_from(args).await,
                "close" => self.close(args),
                other => Err(EvalError::Capability(format!("unknown @udp.{}", other))),
            }
        }
    }

    /// Bind, under exactly the policy `@http.listen` uses for its bind address.
    #[cfg(not(target_arch = "wasm32"))]
    async fn bind(&self, args: Vec<Value>, perms: &PermissionSet) -> Result<Value, EvalError> {
        let addr = args
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:0")
            .to_string();
        crate::http::check_bind_perm(&addr, perms, "bind")?;

        match UdpSocket::bind(&addr).await {
            Ok(sock) => {
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                self.sockets.lock().insert(id, Arc::new(sock));
                Ok(Value::ok(Value::Handle(HostHandle {
                    kind: HANDLE_KIND.into(),
                    id,
                })))
            }
            // A busy port or an unavailable interface is a condition a script can
            // handle, so it is a value — the same choice `@fs` makes for io errors.
            Err(e) => Ok(udp_err("udp.bind", &addr, &e.to_string())),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn local_addr(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let sock = self.socket(args.first())?;
        match sock.local_addr() {
            Ok(a) => Ok(Value::ok(Value::string(a.to_string()))),
            Err(e) => Ok(udp_err("udp.local_addr", "", &e.to_string())),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn send_to(&self, args: Vec<Value>, perms: &PermissionSet) -> Result<Value, EvalError> {
        let sock = self.socket(args.first())?;
        let dest = args
            .get(1)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                EvalError::Message("udp.send_to expects a destination \"host:port\"".into())
            })?
            .to_string();
        // Per-host, exactly as an outbound `@http` request is checked. The grant is
        // matched against the destination as written; a name is resolved afterwards,
        // so `--allow net=example.com` does not become a grant for wherever DNS
        // points that name next.
        let host = crate::http::addr_host(&dest);
        perms.check_net(&host).map_err(EvalError::Permission)?;

        let payload = payload_bytes(args.get(2))?;
        match sock.send_to(&payload, &dest).await {
            Ok(n) => Ok(Value::ok(Value::Int(n as i64))),
            Err(e) => Ok(udp_err("udp.send_to", &dest, &e.to_string())),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn recv_from(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let sock = self.socket(args.first())?;
        let ms = match args.get(1) {
            None | Some(Value::None) => DEFAULT_TIMEOUT_MS,
            Some(v) => v.as_int().ok_or_else(|| {
                EvalError::Message("udp.recv_from expects timeout_ms as an integer".into())
            })?,
        };
        let ms = ms.max(0) as u64;

        let mut buf = vec![0u8; MAX_DATAGRAM];
        let recv = tokio::time::timeout(
            std::time::Duration::from_millis(ms),
            sock.recv_from(&mut buf),
        )
        .await;

        match recv {
            // Nothing arrived in time. Waiting is the normal case for a datagram
            // socket, so this is an `err` value the caller can branch on — not a
            // raise that would end the program.
            Err(_elapsed) => Ok(Value::err(Value::record(vec![
                (Key::String("kind".into()), Value::string("udp.timeout")),
                (
                    Key::String("operation".into()),
                    Value::string("udp.recv_from"),
                ),
                (
                    Key::String("message".into()),
                    Value::string(format!("no datagram within {ms}ms")),
                ),
                (Key::String("timeout_ms".into()), Value::Int(ms as i64)),
            ]))),
            Ok(Err(e)) => Ok(udp_err("udp.recv_from", "", &e.to_string())),
            Ok(Ok((n, from))) => {
                buf.truncate(n);
                let text = String::from_utf8_lossy(&buf).into_owned();
                Ok(Value::ok(Value::record(vec![
                    (Key::String("from".into()), Value::string(from.to_string())),
                    (Key::String("data".into()), Value::Bytes(buf.into())),
                    (Key::String("text".into()), Value::string(text)),
                ])))
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn close(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let id = handle_id(args.first())?;
        // Dropping the last `Arc` closes the file descriptor. A handle that is
        // already gone is not an error: `close` is what a script runs on the way out.
        self.sockets.lock().remove(&id);
        Ok(Value::ok(Value::None))
    }

    /// Clone the socket out from under the lock, so an awaited `recv_from` never
    /// holds it (which would also serialize every other socket, and trip
    /// `clippy::await_holding_lock`).
    #[cfg(not(target_arch = "wasm32"))]
    fn socket(&self, v: Option<&Value>) -> Result<Arc<UdpSocket>, EvalError> {
        let id = handle_id(v)?;
        self.sockets
            .lock()
            .get(&id)
            .cloned()
            .ok_or_else(|| EvalError::Message("udp socket closed or invalid".into()))
    }
}

/// A payload is text or bytes. Anything else is rejected rather than stringified:
/// silently sending `<bytes len=3>` down the wire is worse than a message.
#[cfg(not(target_arch = "wasm32"))]
fn payload_bytes(v: Option<&Value>) -> Result<Vec<u8>, EvalError> {
    match v {
        Some(Value::String(s)) => Ok(s.as_bytes().to_vec()),
        Some(Value::Bytes(b)) => Ok(b.to_vec()),
        Some(other) => Err(EvalError::Message(format!(
            "udp.send_to expects a string or bytes payload, got {}",
            other.type_name()
        ))),
        None => Err(EvalError::Message(
            "udp.send_to expects a string or bytes payload".into(),
        )),
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
            "expected handle {} from @udp.bind",
            HANDLE_KIND
        ))),
    }
}

/// Transport failures are values, in the shape `@http` uses for `net.error`.
#[cfg(not(target_arch = "wasm32"))]
fn udp_err(op: &str, addr: &str, message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("udp.error")),
        (Key::String("operation".into()), Value::string(op)),
        (Key::String("address".into()), Value::string(addr)),
        (Key::String("message".into()), Value::string(message)),
    ]))
}
