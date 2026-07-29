# Installation

You do **not** need the source tree to use Rite for personal scripts and one-liners. Prefer a **binary install** from GitHub Releases; clone only if you are developing Rite itself.

## Quick install (recommended)

```bash
curl -fsSL https://rite.undrc.dev/install | bash
```

Same script:

```bash
curl -fsSL https://rite.undrc.dev/install.sh | bash
```

This will:

1. Detect your OS/CPU (`linux` / `macOS`, `x86_64` / `arm64`)
2. Download the matching archive from **GitHub Releases** ([undercurrent-labs/rite](https://github.com/undercurrent-labs/rite/releases))
3. Verify **SHA-256** against `SHA256SUMS`
4. Install `rite` and `rite-lsp` into **`~/.local/bin`** (override with `RITE_INSTALL_DIR`)

### Pin a version

```bash
curl -fsSL https://rite.undrc.dev/install | RITE_VERSION=v0.1.8 bash
```

Use the latest tag from [Releases](https://github.com/undercurrent-labs/rite/releases) (omit `RITE_VERSION` to install whatever is current).

### Options

| Variable | Default | Meaning |
|----------|---------|---------|
| `RITE_VERSION` | latest release | Tag such as `v0.1.8` |
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
echo '! @console.println("hello from work")' > /tmp/hi.rite
rite run /tmp/hi.rite
# or: rite /tmp/hi.rite
```

### Agent skill, self-update, VS Code

```bash
rite skill install --target all   # Grok / Claude / Cursor skill dirs
rite update --check               # CLI + skill freshness
rite vscode install               # download .vsix + install via code/cursor
```

Details: [Agents & skill](agents.md) · [https://rite.undrc.dev/agents](https://rite.undrc.dev/agents)

### Security notes

- Prefer reading the script once:  
  `curl -fsSL https://rite.undrc.dev/install.sh -o install-rite.sh && less install-rite.sh && bash install-rite.sh`
- Installer **refuses** archives that fail checksum verification
- Binaries are built by CI from tagged commits (see `.github/workflows/release.yml`)

### Windows

The shell installer targets Unix (macOS, Linux, WSL). On native Windows:

1. Open [Releases](https://github.com/undercurrent-labs/rite/releases)
2. Download `rite-x86_64-pc-windows-msvc.zip`
3. Verify against `SHA256SUMS`
4. Put `rite.exe` on your `PATH`

WSL: use the Linux one-liner inside the distro.

### No release yet?

If the installer says there are no releases, either:

- Someone needs to publish a tag (maintainers: `git tag v0.1.8 && git push origin v0.1.8`), or  
- Use [from source](#from-source-contributors) / `cargo install` below, or  
- Use [Studio](https://rite.undrc.dev/studio) in the browser for pure scripts (no CLI)

## Studio only (zero install)

For pure scripts, format, and explore:

**[https://rite.undrc.dev/studio](https://rite.undrc.dev/studio)**

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

Requirements: recent stable Rust (see `rust-toolchain.toml` if present).

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
rite fmt --stdout examples/hello/hello.rite
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
| `rite capabilities` | List host capabilities |

```bash
rite --help
```

## Permissions

Default: **console**, **clock**, **random** allowed; **fs**, **net**, **env**, **process** denied.

```bash
rite run script.rite --allow-all          # demos
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

## Maintainers: publishing a release

```bash
# on main, green CI
git tag v0.1.8
git push origin v0.1.8
# → GitHub Actions "Release" builds assets + publishes the Release
```

Or **Actions → Release → Run workflow** and enter the tag (e.g. `v0.1.8`).

Local package for the current machine only:

```bash
bash scripts/package-release.sh
# → dist/release/rite-$TARGET.tar.gz + SHA256SUMS
```

Site hosts the installer from `scripts/install.sh` (copied into `apps/rite-web/public/` on `pnpm site:build`).

## Next

[First script](first-script.md) — hello world and dialects.
