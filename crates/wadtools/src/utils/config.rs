use camino::{Utf8Path, Utf8PathBuf};
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct Config {
    /// Whether to show progress bars
    pub show_progress: Option<bool>,
    /// Optional custom directory to recursively load hashtable files from
    pub hashtable_dir: Option<String>,
}

impl Config {
    pub fn resolve_show_progress(&self, cli_progress: Option<bool>) -> bool {
        match cli_progress {
            Some(v) => v,
            None => self.show_progress.unwrap_or(true),
        }
    }
}

pub fn default_config_path() -> Utf8PathBuf {
    let exe_path = std::env::current_exe().unwrap();
    let mut dir = exe_path.parent().unwrap().to_path_buf();
    dir.push("wadtools.toml");

    Utf8PathBuf::from_path_buf(dir).unwrap()
}

pub fn load_config(path: Option<&Utf8Path>) -> Result<Config> {
    let final_path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path(),
    };

    if final_path.exists() {
        let contents = fs::read_to_string(final_path.as_std_path())?;
        let cfg: Config = toml::from_str(&contents)?;
        Ok(cfg)
    } else {
        Ok(Config::default())
    }
}

pub fn save_config(path: Option<&Utf8Path>, cfg: &Config) -> Result<()> {
    let final_path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path(),
    };
    let toml_str = toml::to_string_pretty(cfg)?;
    fs::write(final_path.as_std_path(), toml_str)?;
    Ok(())
}

/// Loads config and ensures a file exists by writing defaults if missing.
/// Returns the loaded config and the resolved path it was loaded/saved from.
pub fn load_or_create_config(path: Option<&Utf8Path>) -> Result<(Config, Utf8PathBuf)> {
    let final_path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path(),
    };

    let cfg = if final_path.exists() {
        load_config(Some(final_path.as_path()))?
    } else {
        let cfg = Config::default();
        save_config(Some(final_path.as_path()), &cfg)?;
        cfg
    };

    Ok((cfg, final_path))
}

/// Resolves progress (CLI overrides config). If CLI provided, persist updated value.
pub fn resolve_and_persist_progress(
    cfg: &mut Config,
    path: &Utf8Path,
    cli_progress: Option<bool>,
) -> Result<bool> {
    let show_progress = cfg.resolve_show_progress(cli_progress);
    if cli_progress.is_some() {
        cfg.show_progress = Some(show_progress);
        save_config(Some(path), cfg)?;
    }
    Ok(show_progress)
}
