# 13 Protocol buffers

```bash
rite run examples/13-proto/main.rite --allow fs:read=.
# or: --allow-all
```

Compiles `user.proto` at run time — no `protoc`, no build step — then encodes a
message and reads it back.

Two things to notice: a protobuf enum arrives as a Rite atom, so `#PRO` matches
like any literal; and the decoded record holds only the fields the message
actually set, so an unset `nickname` is `none` rather than `""`.

`@proto` is native-only in practice: the browser build omits it, because the
schema compiler would roughly double the WASM bundle.
