# 12 MCP

`main.rite` is a server: tools, a resource and a prompt, over stdio.

```bash
rite run examples/12-mcp/main.rite
```

`client.rite` is the other direction, and it drives `main.rite` as its subprocess, so
the two together are a complete round trip:

```bash
rite run examples/12-mcp/client.rite --allow process
```

Run it from the repository root — the server path in `client.rite` is relative to the
working directory. `--allow process` is what a stdio connection needs, because it
starts a program; an HTTP server needs `--allow net=<host>` instead.
