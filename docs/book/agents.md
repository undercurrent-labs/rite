# Agents, skill, and self-update

Rite ships an **agent skill** (prompt + machine-readable grammar/capabilities) so coding agents write valid Rite instead of inventing syntax. The CLI can install that skill, self-update binaries, and fetch the VS Code extension.

## Install the skill

```bash
# Grok (default)
rite skill install

# All common agent hosts
rite skill install --target all

# Project-local (.grok/skills/rite)
rite skill install --target project

# Custom directory
rite skill install --dir ~/.my-agent/skills/rite
```

Status and paths:

```bash
rite skill status
rite skill path
rite skill update
```

The skill is cached under `~/.local/share/rite/skill/rite`. Install metadata (version, last pull time, destinations) lives in `~/.config/rite/config.json`.

### Without the CLI

Download from the site or a GitHub Release:

```bash
curl -fsSL https://rite.foo/skill/rite-agent-skill.tar.gz -o skill.tgz
mkdir -p ~/.grok/skills
tar -xzf skill.tgz -C ~/.grok/skills   # → ~/.grok/skills/rite
```

Site: [rite.foo/agents](https://rite.foo/agents)

## Update CLI and skill

```bash
rite update --check    # report only; exit 1 if anything is behind
rite update            # install newer CLI if any; refresh skill when needed
rite self-update       # alias of update
```

`--check` prints:

- Installed CLI vs latest GitHub release  
- Skill last pull / version vs the skill channel on that release  
- Whether the local skill cache is present  

Config fields updated on every check: `last_update_check`, `last_cli_version_seen`, `last_skill_version_seen`.

## VS Code / Cursor extension

```bash
rite vscode info                 # asset URLs, sizes, instructions
rite vscode download --out ./rite.vsix
rite vscode install              # download + code/cursor --install-extension
rite vscode install --editor cursor
rite vscode install --download-only
```

Point the extension at absolute binaries if the GUI `PATH` is thin:

```json
{
  "rite.lspPath": "/home/you/.local/bin/rite-lsp",
  "rite.binaryPath": "/home/you/.local/bin/rite"
}
```

## For agents

1. Load `SKILL.md` and `machine/*.json` from the skill tree.  
2. Do not invent syntax not listed in aliases/grammar.  
3. Mark effectful host calls with `!` / `do`.  
4. Prefer `rite check` / `rite fmt` / `rite run --allow-all` when available.  
5. Introspect: `rite describe language --json`.

## Regenerating the skill (contributors)

```bash
rite docs agent --output skills/rite
bash scripts/package-skill.sh dist/skill
```
