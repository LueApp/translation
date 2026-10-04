use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persistent user configuration, stored at ~/.config/ai-translate/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Active backend: "mymemory" (free, no key) | "ai" (your key) | "libre" | "google"
    pub provider: String,
    /// Ordered sources. Empty keeps the legacy single-provider settings active.
    pub sources: Vec<SourceConfig>,
    /// source language code, or "auto"
    pub source_lang: String,
    /// target language code (e.g. "zh-CN", "en")
    pub target_lang: String,
    /// tesseract language(s) for OCR, e.g. "eng" or "eng+chi_sim"
    pub ocr_langs: String,

    /// OpenAI-compatible AI backend (DeepSeek / Kimi / GLM / Qwen / Doubao / OpenAI…).
    /// `ai_base_url` must include the version path; "/chat/completions" is appended.
    pub ai_base_url: String,
    pub ai_model: String,
    pub ai_key: String,

    /// LibreTranslate endpoint + optional key (used when provider = "libre").
    pub libre_url: String,
    pub libre_key: String,

    /// Optional HTTP/SOCKS proxy for ALL backends, e.g.
    /// "http://127.0.0.1:7890" or "socks5://127.0.0.1:7891".
    /// Empty uses environment proxy settings; "direct" disables proxies.
    /// Needed to reach providers blocked on your network (e.g. Google).
    pub proxy_url: String,

    pub font_size: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            provider: "mymemory".to_string(),
            sources: Vec::new(),
            source_lang: "auto".to_string(),
            target_lang: "zh-CN".to_string(),
            ocr_langs: "eng".to_string(),
            ai_base_url: "https://api.deepseek.com/v1".to_string(),
            ai_model: "deepseek-chat".to_string(),
            ai_key: String::new(),
            // Public libretranslate.com now requires an API key; this mirror is
            // free and keyless. Override per `libre_url` if you self-host.
            libre_url: "https://translate.disroot.org".to_string(),
            libre_key: String::new(),
            proxy_url: String::new(),
            font_size: 16.0,
        }
    }
}

/// One translation attempt. AI and Libre entries can each use separate credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub provider: String,
    pub label: String,
    pub timeout_secs: u64,
    pub ai_base_url: String,
    pub ai_model: String,
    pub ai_key: String,
    pub libre_url: String,
    pub libre_key: String,
    /// Empty inherits the global proxy; "direct" bypasses all proxies.
    pub proxy_url: String,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            provider: "mymemory".into(),
            label: String::new(),
            timeout_secs: 40,
            ai_base_url: String::new(),
            ai_model: String::new(),
            ai_key: String::new(),
            libre_url: String::new(),
            libre_key: String::new(),
            proxy_url: String::new(),
        }
    }
}

impl Config {
    pub fn dir() -> Result<PathBuf> {
        let base = directories::BaseDirs::new().context("no home dir")?;
        Ok(base.config_dir().join("ai-translate"))
    }

    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.toml"))
    }

    /// Load config, creating a default file on first run.
    pub fn load() -> Result<Config> {
        let path = Self::path()?;
        if !path.exists() {
            let cfg = Config::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir()?;
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let text = toml::to_string_pretty(self)?;
        std::fs::write(Self::path()?, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_and_source_sequence_round_trip() {
        let legacy: Config = toml::from_str("provider = 'ai'\nai_key = 'old-key'").unwrap();
        assert!(legacy.sources.is_empty());
        let mut cfg = legacy;
        cfg.sources.push(SourceConfig {
            provider: "ai".into(),
            label: "Backup".into(),
            timeout_secs: 7,
            ai_key: "new-key".into(),
            proxy_url: "direct".into(),
            ..SourceConfig::default()
        });
        let saved = toml::to_string_pretty(&cfg).unwrap();
        let restored: Config = toml::from_str(&saved).unwrap();
        assert_eq!(restored.provider, "ai");
        assert_eq!(restored.sources[0].label, "Backup");
        assert_eq!(restored.sources[0].timeout_secs, 7);
        assert_eq!(restored.sources[0].ai_key, "new-key");
        assert_eq!(restored.sources[0].proxy_url, "direct");
    }
}
