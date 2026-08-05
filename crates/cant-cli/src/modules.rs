//! Where a `use` comes from, and where it is found.
//!
//! A Cant program's only import form is a leading `use NAME`
//! (`crates/cant-syntax/src/parser.rs`), and resolution is entirely Rite's.
//! That is right for a file and wrong for the two forms this tool is actually
//! used in: `cant -e '…'` and the REPL have no file to put a `use` line at the
//! top of, and Rite's module search had no root to add — it looks beside the
//! script, then in whatever roots it was handed, then in the process's working
//! directory (`crates/rite-sem/src/modules.rs`), and nothing ever handed it
//! any.
//!
//! # Three layers, one precedence
//!
//! **flag > environment > config file.** A flag is this invocation, an
//! environment variable is this shell, a config file is this directory tree;
//! the more specific one wins, and `--no-default-use` turns the outer two off
//! entirely for a run that has to be reproducible.
//!
//! Later layers do not replace earlier ones for *modules* — a `--use` adds to
//! what `CANT_USE` asked for — because "make these available" composes and
//! "use exactly these" does not. Order is preserved, duplicates are dropped.
//!
//! # What a config file may not do
//!
//! **It may not grant permissions.** `cant.toml` is discovered by walking up
//! from the working directory, so honouring an `allow = [...]` in it would mean
//! `cd` into a directory could widen what a program is permitted to do, and
//! cloning a repository would be enough to arrange that. Permissions come from
//! the command line, where the person running the program can see them. If
//! someone adds a key here, this is why they should not.

use std::path::{Path, PathBuf};

/// The file name looked for, in order, at each level walking up.
const CONFIG_NAMES: &[&str] = &["cant.toml", ".cant.toml"];

/// How many levels up to look. Deep enough for a monorepo, shallow enough that
/// a stray file in `/home` or `/` cannot silently configure every run.
const MAX_CONFIG_DEPTH: usize = 24;

/// The resolved module settings for one invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Modules {
    pub uses: Vec<String>,
    pub roots: Vec<PathBuf>,
    /// Which layer supplied each `use`, so a module that cannot be found can
    /// say where the request came from rather than surfacing as an `E026`
    /// about generated Rite.
    pub origins: Vec<(String, Origin)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Flag,
    Environment,
    Config,
}

impl Origin {
    pub fn describe(self, config: Option<&Path>) -> String {
        match self {
            Origin::Flag => "`--use`".into(),
            Origin::Environment => "`CANT_USE`".into(),
            Origin::Config => match config {
                Some(path) => format!("`use` in {}", path.display()),
                None => "the config file".into(),
            },
        }
    }
}

impl Modules {
    pub fn origin_of(&self, name: &str) -> Option<Origin> {
        self.origins
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, o)| *o)
    }
}

/// What a `cant.toml` may say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub uses: Vec<String>,
    pub roots: Vec<PathBuf>,
}

/// Resolve the three layers.
///
/// `from` is the directory the walk up starts at — the working directory, or
/// the script's own directory for `cant run file.cant`, so a `cant.toml` beside
/// a program applies to it however it was invoked.
pub fn resolve(
    flag_uses: &[String],
    flag_roots: &[PathBuf],
    no_defaults: bool,
    from: &Path,
) -> Result<(Modules, Option<PathBuf>), String> {
    let mut modules = Modules::default();
    let mut config_path = None;

    if !no_defaults {
        if let Some((path, config)) = find_config(from)? {
            add(&mut modules, &config.uses, Origin::Config);
            // Roots in a config file are relative to the file, not to the
            // caller: `module-roots = ["./lib"]` means the `lib` beside the
            // config, or it would mean something different from every
            // directory it was run in.
            let base = path.parent().unwrap_or(Path::new("."));
            modules
                .roots
                .extend(config.roots.iter().map(|r| base.join(r)));
            config_path = Some(path);
        }
        let (env_uses, env_roots) = from_environment();
        add(&mut modules, &env_uses, Origin::Environment);
        modules.roots.extend(env_roots);
    }
    add(&mut modules, flag_uses, Origin::Flag);
    modules.roots.extend(flag_roots.iter().cloned());

    modules.roots.dedup();
    Ok((modules, config_path))
}

/// Add names not already present, recording where each came from.
///
/// First mention wins the origin, and the flag layer is added last, so a module
/// named in both a config file and on the command line is reported as the
/// config's. That is the useful direction: the flag is visible in the shell
/// history, the config file is the one that needs finding.
fn add(modules: &mut Modules, names: &[String], origin: Origin) {
    for name in names {
        if name.is_empty() {
            continue;
        }
        if !modules.uses.contains(name) {
            modules.uses.push(name.clone());
            modules.origins.push((name.clone(), origin));
        }
    }
}

/// `CANT_USE=a,b` and `CANT_MODULE_PATH=dir1:dir2`.
fn from_environment() -> (Vec<String>, Vec<PathBuf>) {
    let uses = std::env::var("CANT_USE")
        .ok()
        .map(|v| split_list(&v))
        .unwrap_or_default();
    let roots = std::env::var_os("CANT_MODULE_PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    (uses, roots)
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The nearest config file at or above `from`, and what it says.
fn find_config(from: &Path) -> Result<Option<(PathBuf, Config)>, String> {
    let mut dir = from.to_path_buf();
    for _ in 0..MAX_CONFIG_DEPTH {
        for name in CONFIG_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                let text = std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("cannot read {}: {e}", candidate.display()))?;
                let config =
                    parse_config(&text).map_err(|e| format!("{}: {e}", candidate.display()))?;
                return Ok(Some((candidate, config)));
            }
        }
        if !dir.pop() {
            break;
        }
    }
    Ok(None)
}

/// Parse the two keys a `cant.toml` may carry.
///
/// A hand-written reader rather than a TOML dependency: two keys, both lists of
/// strings, and `cant-cli` has no TOML parser today. An unknown key is an
/// **error**, not a shrug — a typo in `module-roots` that was silently ignored
/// would present as "module not found" pointing at generated Rite.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let mut config = Config::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            return Err(format!(
                "line {}: this file has no tables — `use` and `module-roots` are top-level keys",
                n + 1
            ));
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected `key = [...]`", n + 1));
        };
        let items = parse_list(value.trim())
            .ok_or_else(|| format!("line {}: expected a list of strings", n + 1))?;
        match key.trim() {
            "use" => config.uses.extend(items),
            "module-roots" => config.roots.extend(items.into_iter().map(PathBuf::from)),
            "allow" | "allow-all" | "deny" => {
                return Err(format!(
                    "line {}: permissions cannot be set from a config file — \
                     a file found by walking up from the working directory must not be \
                     able to widen what a program may do. Pass `--{}` on the command line.",
                    n + 1,
                    key.trim()
                ))
            }
            other => {
                return Err(format!(
                    "line {}: unknown key `{other}` — expected `use` or `module-roots`",
                    n + 1
                ))
            }
        }
    }
    Ok(config)
}

/// `["a", "b"]`, or a bare `"a"` for the one-item case.
fn parse_list(value: &str) -> Option<Vec<String>> {
    if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        let mut out = Vec::new();
        for item in inner.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            out.push(unquote(item)?);
        }
        return Some(out);
    }
    Some(vec![unquote(value)?])
}

fn unquote(item: &str) -> Option<String> {
    let inner = item
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| item.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))?;
    Some(inner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_file_carries_two_keys_and_nothing_else() {
        let config = parse_config(
            r#"
            # a comment
            use = ["helpers", "math"]
            module-roots = ["./lib"]
            "#,
        )
        .expect("parses");
        assert_eq!(config.uses, vec!["helpers", "math"]);
        assert_eq!(config.roots, vec![PathBuf::from("./lib")]);
    }

    /// A typo silently ignored would present as "module not found" pointing at
    /// generated Rite, which is the least actionable message this tool has.
    #[test]
    fn an_unknown_key_is_an_error() {
        let e = parse_config("module_roots = [\"./lib\"]").expect_err("refused");
        assert!(e.contains("unknown key `module_roots`"), "{e}");
        assert!(e.contains("module-roots"), "{e}");
    }

    /// The security property this file exists to state: a file found by walking
    /// up from the working directory must not be able to grant anything.
    #[test]
    fn permissions_cannot_come_from_a_config_file() {
        for key in ["allow", "allow-all", "deny"] {
            let e = parse_config(&format!("{key} = [\"fs:write=/\"]")).expect_err("refused");
            assert!(e.contains("permissions cannot be set"), "{key}: {e}");
            assert!(e.contains("command line"), "{key}: {e}");
        }
    }

    #[test]
    fn a_list_takes_either_quoting_and_a_bare_single_item() {
        assert_eq!(
            parse_list(r#"["a", 'b']"#),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(parse_list(r#""solo""#), Some(vec!["solo".into()]));
        assert_eq!(parse_list("[]"), Some(vec![]));
        assert_eq!(parse_list("unquoted"), None);
    }

    #[test]
    fn an_environment_list_splits_on_commas_and_spaces() {
        assert_eq!(split_list("a,b"), vec!["a", "b"]);
        assert_eq!(split_list(" a , , b "), vec!["a", "b"]);
        assert_eq!(split_list(""), Vec::<String>::new());
    }

    /// Precedence, and the reporting that goes with it: a module named twice is
    /// listed once, attributed to the layer that would be hardest to find.
    #[test]
    fn layers_compose_and_remember_where_each_came_from() {
        let mut modules = Modules::default();
        add(&mut modules, &["helpers".into()], Origin::Config);
        add(
            &mut modules,
            &["helpers".into(), "math".into()],
            Origin::Flag,
        );
        assert_eq!(modules.uses, vec!["helpers", "math"]);
        assert_eq!(modules.origin_of("helpers"), Some(Origin::Config));
        assert_eq!(modules.origin_of("math"), Some(Origin::Flag));
        assert_eq!(modules.origin_of("absent"), None);
    }

    #[test]
    fn no_defaults_uses_only_the_flags() {
        let dir = std::env::temp_dir().join("cant-modules-no-defaults");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("cant.toml"), "use = [\"fromfile\"]\n").expect("write");
        let (modules, config) = resolve(&["fromflag".into()], &[], true, &dir).expect("resolves");
        assert_eq!(modules.uses, vec!["fromflag"]);
        assert_eq!(config, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A root in a config file is relative to the file, not to wherever the
    /// command happened to be run.
    #[test]
    fn a_config_root_is_relative_to_the_config() {
        let dir = std::env::temp_dir().join("cant-modules-relative/nested");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let top = dir.parent().expect("parent");
        std::fs::write(top.join("cant.toml"), "module-roots = [\"lib\"]\n").expect("write");
        let (modules, config) = resolve(&[], &[], false, &dir).expect("resolves");
        assert_eq!(config.as_deref(), Some(top.join("cant.toml").as_path()));
        assert_eq!(modules.roots, vec![top.join("lib")]);
        let _ = std::fs::remove_dir_all(top);
    }
}
