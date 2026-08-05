# Installation

You do **not** need the source tree to run Rite scripts. Prefer a **binary install** from GitHub Releases; clone only if you are developing Rite itself.

## Quick install (recommended)

```bash
curl -fsSL https://rite.foo/install | bash
```

Same script:

```bash
curl -fsSL https://rite.foo/install.sh | bash
```

This will:

1. Detect your OS/CPU (`linux` / `macOS`, `x86_64` / `arm64`)
2. Download the matching archive from **GitHub Releases** ([undercurrent-labs/rite](https://github.com/undercurrent-labs/rite/releases))
3. Verify **SHA-256** against `SHA256SUMS`
4. Install `rite` and `rite-lsp` into **`~/.local/bin`** (override with `RITE_INSTALL_DIR`)

### Pin a version

```bash
curl -fsSL https://rite.foo/install | RITE_VERSION=v0.9.1 bash
```

Use the latest tag from [Releases](https://github.com/undercurrent-labs/rite/releases) (omit `RITE_VERSION` to install whatever is current).

### Options

| Variable | Default | Meaning |
|----------|---------|---------|
| `RITE_VERSION` | latest release | Tag such as `v0.9.1` |
| `RITE_INSTALL_DIR` | `$HOME/.local/bin` | Where binaries go |
| `INSTALL_LSP` | `1` | Set `0` to skip `rite-lsp` |
| `RITE_REPO` | `undercurrent-labs/rite` | GitHub repo for assets |
| `RITE_DRY_RUN` | `0` | Set `1` to print plan only |

### PATH

If the installer says the directory is not on your `PATH`:

```bash
# bash / zsh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc   # or ~/.zshrc
source ~/.bashrc
```

### Verify

```bash
rite version --verbose
echo '! @console.println("hello from Rite")' > /tmp/hi.rite
rite run /tmp/hi.rite
# or: rite /tmp/hi.rite
```

### Agent skill, self-update, VS Code

```bash
rite skill install --target all   # Grok / Claude / Cursor skill dirs
rite update --check               # CLI + skill freshness
rite vscode install               # download .vsix + install via code/cursor
```

Details: [Agents & skill](agents.md) · [https://rite.foo/agents](https://rite.foo/agents)

Opening a `.rite` file gives you highlighting, inline errors from `rite-lsp`,
and a **Run** lens above the program and above `main` — one click, no terminal.
The lens spans come from the analysis the server already ran, so one never
appears over a `def` inside a string.

> **Lenses run without permissions.** Clicking Run on a program you are reading
> should not hand it your filesystem, so a lens grants nothing. A program that
> names capabilities reads `▶ Run (ungranted)` and the tooltip says which ones;
> set `rite.codeLens.allowAll` if you would rather they ran with `--allow-all`.

The **Rite Noir** theme ships with it — the syntax palette on a near-black base,
the same twelve colours the site and `rite render` use. Pick it with
<kbd>Ctrl/Cmd</kbd>+<kbd>K</kbd> <kbd>Ctrl/Cmd</kbd>+<kbd>T</kbd>.

### Security notes

- Prefer reading the script once:  
  `curl -fsSL https://rite.foo/install.sh -o install-rite.sh && less install-rite.sh && bash install-rite.sh`
- Installer **refuses** archives that fail checksum verification
- Binaries are built by CI from tagged commits (see `.github/workflows/release.yml`)

### Windows

**No Windows binary is published.** Rite builds and runs on Windows, but CI no longer
tests it on every change, and shipping a binary nothing exercises would mean a regression
reaching you with nothing in its way. Two supported routes:

- **WSL** — use the Linux one-liner inside the distro. This is the recommended path.
- **Build from source** — `cargo install --path crates/rite-cli` in a checkout. You get
  the same program; you are the one testing it.

`rite update` says the same rather than looking for an archive that is not there.

### If the installer cannot find a release

No binary matches every platform — see **Windows** above, and new architectures land in
Releases before they land in the installer's detection table. Either:

- Build [from source](#from-source-contributors) or use `cargo install` below, or  
- Use [Studio](https://rite.foo/studio) in the browser for pure scripts (no CLI)

## Studio only (zero install)

For pure scripts, format, and explore:

**[https://rite.foo/studio](https://rite.foo/studio)**

No binary, no Rust toolchain. Full FS/HTTP/process still needs the CLI.

---

## From source (contributors)

```bash
git clone https://github.com/undercurrent-labs/rite.git
cd rite
cargo build -p rite-cli -p rite-lsp --release
export PATH="$PWD/target/release:$PATH"
rite version --verbose
```

Requirements: the Rust toolchain pinned in `rust-toolchain.toml` (1.97.1), which `rustup`
installs automatically from within the checkout.

### cargo install from git

If you have Rust but do not want a full working tree checkout for daily use:

```bash
cargo install --git https://github.com/undercurrent-labs/rite --locked --package rite-cli
# optional:
cargo install --git https://github.com/undercurrent-labs/rite --locked --package rite-lsp
```

This **compiles on your machine** (slower first install) and still needs a network + toolchain. Prefer the **curl installer** when Releases exist.

### Verify from a clone

```bash
rite run examples/01-values/main.rite --allow-all
rite check examples/hello/hello.rite
rite convert examples/hello/hello.rite --to glyph --stdout
```

## CLI surface

| Command | Purpose |
|---------|---------|
| `rite run <file>` | Interpret a script |
| `rite check <file>` | Parse + resolve |
| `rite fmt [file]` | Format (`--ascii` for ASCII dialect) |
| `rite build <file>` | Compile to native binary |
| `rite repl` | Interactive |
| `rite studio` | Local Studio API host |
| `rite docs build` | Generate reference docs |
| `rite doc [path]` | Document your own scripts' `///` comments |
| `rite render <file>` | Draw highlighted source as SVG or PNG |
| `rite capabilities` | List host capabilities |

```bash
rite --help
```

## Permissions

Default: **console**, **clock**, **random** allowed; **fs**, **net**, **env**, **process**
and **db** denied. Any default can be revoked with `--deny`.

```bash
rite run script.rite --allow-all          # trusted scripts only
rite run script.rite --allow fs:read=./data
```

See [Effects and capabilities](effects.md).

## Editors

**LSP** (installed next to `rite` by the installer):

```bash
rite-lsp   # stdio language server
```

**VS Code** (from a clone, development host):

```bash
cd editors/vscode && npm install && npm run compile
```

## Next

[First script](first-script.md) — hello world and dialects.
