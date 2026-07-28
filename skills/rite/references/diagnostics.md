# Diagnostics

| Code | Summary |
|------|---------|
| E020 | Undefined name |
| E021 | Effect marker `!` required |
| E022 | Duplicate binding |
| E023 | Assign to immutable |
| E024 | Circular import |
| E026 | Module not found |
| E040 | Permission denied |
| E010–E013 | Parse errors |

Full pages: `docs/diagnostics/`.

```bash
rite check --json-errors file.rite
rite describe diagnostic E021 --json
```
