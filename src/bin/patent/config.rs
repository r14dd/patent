use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub model: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patent").join("config.toml"))
}

pub fn load() -> anyhow::Result<Config> {
    let path = match config_path() {
        Some(p) => p,
        None => return Ok(Config::default()),
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields() {
        let cfg: Config = toml::from_str(
            r#"
            model    = "gpt-4o-mini"
            api_base = "https://api.openai.com/v1"
            api_key  = "sk-test"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(cfg.api_base.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(cfg.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn empty_file_is_ok() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.model.is_none());
        assert!(cfg.api_base.is_none());
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn partial_fields_ok() {
        let cfg: Config = toml::from_str(r#"model = "qwen2.5:3b""#).unwrap();
        assert_eq!(cfg.model.as_deref(), Some("qwen2.5:3b"));
        assert!(cfg.api_base.is_none());
    }

    #[test]
    fn unknown_field_is_error() {
        assert!(toml::from_str::<Config>(r#"typo_key = "oops""#).is_err());
    }
}
