//! Local Rite config + skill cache under the user's config/data dirs.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DEFAULT_REPO: &str = "undercurrent-labs/rite";
const SITE_BASE: &str = "https://rite.undrc.dev";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiteConfig {
    #[serde(default)]
    pub skill: SkillState,
    #[serde(default)]
    pub last_update_check: Option<String>,
    #[serde(default)]
    pub last_cli_version_seen: Option<String>,
    #[serde(default)]
    pub last_skill_version_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillState {
    /// ISO-8601 time of last successful skill install/update.
    pub installed_at: Option<String>,
    /// Release tag or package version last installed (e.g. `v0.1.7`).
    pub version: Option<String>,
    /// Content fingerprint (sha256 of archive or of version.json).
    pub fingerprint: Option<String>,
    /// Where the skill was fetched from.
    pub source: Option<String>,
    /// Install destinations written last time (absolute paths).
    #[serde(default)]
    pub install_paths: Vec<String>,
}

impl RiteConfig {
    pub fn load() -> Self {
        let path = config_file();
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        fs::write(&path, text)?;
        Ok(())
    }
}

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("rite")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| home_dir().join(".local").join("share"))
        .join("rite")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.json")
}

/// Cached skill package extracted here before linking into agent skill dirs.
pub fn skill_cache_dir() -> PathBuf {
    data_dir().join("skill").join("rite")
}

pub fn default_repo() -> &'static str {
    DEFAULT_REPO
}

pub fn site_base() -> &'static str {
    SITE_BASE
}

pub fn now_iso() -> String {
    // UTC-ish simple timestamp without extra deps
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Format as approximate ISO; good enough for ordering/display
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    let s = rem % 60;
    // 1970-01-01 + days — keep simple RFC3339-ish without calendar lib
    format!("unix:{days}dT{hours:02}:{mins:02}:{s:02}Z")
}

/// Expand `~` at the start of a path.
pub fn expand_user(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    if p == "~" {
        return home_dir();
    }
    PathBuf::from(p)
}
