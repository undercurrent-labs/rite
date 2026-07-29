# 08 Middleware

Built-in access log / recover plus a **custom Bearer auth** middleware.

```bash
rite run examples/08-middleware/main.rite --allow-all
```

Watch the `listening on` line, then:

```bash
# rejected (401)
curl -sS -i http://127.0.0.1:PORT/secret

# allowed (200)
curl -sS -i -H 'Authorization: Bearer secret' http://127.0.0.1:PORT/secret
```

Replace `PORT` with the ephemeral port Rite printed.
