# Tutorials

Project-shaped guides. Each one builds a small working thing end to end and
explains the decisions along the way — as opposed to [the book](../book/README.md),
which covers one topic per chapter and is meant to be read in order.

Every example in every tutorial was run to produce the output printed beside it.

| | Tutorial | You build | Needs |
|---|---|---|---|
| 1 | [Reshaping JSON](json-pipeline.md) | A report generator: read orders, filter, group, rank, write | nothing |
| 2 | [Auditing a directory](fs-audit.md) | A CLI tool that sizes a directory and flags stale files | nothing |

More are planned — an HTTP service with real routes, a CLI with argument
parsing, compiling to a binary, embedding Rite in a Rust program, and a DNS
resolver over `@udp`.

## Before you start

Install Rite ([Installation](../book/installation.md)) and read
[First script](../book/first-script.md) if you have not written one — the
tutorials assume you know how to run a file and what `←`, `→` and `⟦ ⟧` mean.

Every tutorial uses the glyph dialect. If you prefer ASCII, `rite fmt --ascii`
converts any of them in place; the two are the same program.
