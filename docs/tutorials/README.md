# Tutorials

Project-shaped guides. Each one builds a small working thing end to end and
explains the decisions along the way — as opposed to [the book](../book/README.md),
which covers one topic per chapter and is meant to be read in order.

Every example in every tutorial was run to produce the output printed beside it.

Each tutorial ends with a complete script, and **CI runs that script on every
build** against fixtures in `fixtures/`, comparing what it prints to what the page
says it prints. A tutorial that stops being true fails the build.

| | Tutorial | You build | Needs |
|---|---|---|---|
| 1 | [Reshaping JSON](json-pipeline.md) | A report generator: read orders, filter, group, rank, write | nothing |
| 2 | [Building a CLI](cli-tool.md) | A command-line greeter: arguments, flags, usage errors | nothing |
| 3 | [Testing what you built](testing-what-you-built.md) | A test suite, and the permission posture of `rite test` | Building a CLI |
| 4 | [An HTTP service with real routes](http-service.md) | A JSON API with routes, a body, and a client that proves it | nothing |
| 5 | [A DNS resolver over `@udp`](dns-resolver.md) | A DNS client: wire-format bytes, one datagram, an address | a nameserver |
| 6 | [Compiling to a binary](compiling-a-binary.md) | A native executable, and permissions baked in at build time | a Rust toolchain |
| 7 | [Auditing a directory](fs-audit.md) | A CLI tool that sizes a directory and flags stale files | nothing |

More are planned — embedding Rite in a Rust program.

## Before you start

Install Rite ([Installation](../book/installation.md)) and read
[First script](../book/first-script.md) if you have not written one — the
tutorials assume you know how to run a file and what `←`, `→` and `⟦ ⟧` mean.

Every tutorial uses the glyph dialect. If you prefer ASCII, `rite fmt --ascii`
converts any of them in place; the two are the same program.
