//! ワークスペース発見と banto.toml 設定。設定はすべて省略可(cli_spec_v1.md)。

use std::path::{Path, PathBuf};

pub type R<T> = Result<T, String>;

pub const DEFAULT_LEDGER: &str = "ledger/ledger.jsonl";
pub const DEFAULT_OBJECTS: &str = "ledger/objects";

pub struct Workspace {
    pub root: PathBuf,
    pub actor: String,
    pub stall_days: i64,
    pub habit_stall_days: i64,
    pub wip_limit: usize,
    pub ledger_rel: String,
    pub objects_rel: String,
}

impl Workspace {
    pub fn ledger_path(&self) -> PathBuf {
        self.root.join(&self.ledger_rel)
    }

    pub fn objects_path(&self) -> PathBuf {
        self.root.join(&self.objects_rel)
    }

    pub fn brief_config(&self) -> banto_kernel::BriefConfig {
        banto_kernel::BriefConfig {
            now: chrono::Local::now().fixed_offset(),
            stall_days: self.stall_days,
            habit_stall_days: self.habit_stall_days,
            wip_limit: self.wip_limit,
        }
    }
}

pub fn default_actor() -> String {
    std::env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// ワークスペース発見: `--root` があればそこ、無ければ cwd から上へ banto.toml を探す。
pub fn discover(root_flag: Option<&Path>, actor_flag: Option<&str>) -> R<Workspace> {
    let root = match root_flag {
        Some(r) => {
            let r = std::fs::canonicalize(r)
                .map_err(|e| format!("--root を解決できない: {}: {e}", r.display()))?;
            if !r.join("banto.toml").is_file() {
                return Err(format!(
                    "banto.toml が見つからない: {}(`banto init` でワークスペースを作る)",
                    r.display()
                ));
            }
            r
        }
        None => {
            let cwd = std::env::current_dir().map_err(|e| format!("cwd を取得できない: {e}"))?;
            let mut dir = cwd;
            loop {
                if dir.join("banto.toml").is_file() {
                    break dir;
                }
                if !dir.pop() {
                    return Err(
                        "banto.toml が見つからない。`banto init` でワークスペースを作る".into(),
                    );
                }
            }
        }
    };
    let raw = std::fs::read_to_string(root.join("banto.toml"))
        .map_err(|e| format!("banto.toml を読めない: {e}"))?;
    let table: toml::Table = raw
        .parse()
        .map_err(|e| format!("banto.toml の解析に失敗: {e}"))?;
    let int = |key: &str, default: i64| -> i64 {
        table
            .get(key)
            .and_then(|v| v.as_integer())
            .unwrap_or(default)
    };
    let paths = table.get("paths").and_then(|v| v.as_table());
    let path_of = |key: &str, default: &str| -> String {
        paths
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    };
    let actor = actor_flag
        .map(str::to_owned)
        .or_else(|| {
            table
                .get("actor")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(default_actor);
    Ok(Workspace {
        actor,
        stall_days: int("stall_days", 4),
        habit_stall_days: int("habit_stall_days", 2),
        wip_limit: int("wip_limit", 2).max(0) as usize,
        ledger_rel: path_of("ledger", DEFAULT_LEDGER),
        objects_rel: path_of("objects", DEFAULT_OBJECTS),
        root,
    })
}
