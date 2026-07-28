# Capabilities

| Capability | Notes |
|------------|--------|
| @console | print, println, warn, error (effectful) |
| @fs | read/write; needs fs:read / fs:write |
| @json | decode/encode; pure decode |
| @http | listen + routes; virtual in browser |
| @clock | now, sleep (nondeterministic) |
| @env | needs env allowlist |
| @process | native only; denied in browser |
| @random | seedable |
| @game | text RPG |
| @store | in-memory KV |

```bash
rite describe capability @http --json
rite capabilities
```
