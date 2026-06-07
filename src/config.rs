use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persistent user configuration, stored at ~/.config/ai-translate/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Active backend: "mymemory" (free, no key) | "ai" (your key) | "libre" | "google"
    pub provider: String,
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

    pub font_size: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            provider: "mymemory".to_string(),
            source_lang: "auto".to_string(),
            target_lang: "zh-CN".to_string(),
            ocr_langs: "eng".to_string(),
            ai_base_url: "https://api.deepseek.com/v1".to_string(),
            ai_model: "deepseek-chat".to_string(),
            ai_key: String::new(),
            libre_url: "https://libretranslate.com".to_string(),
            libre_key: String::new(),
            font_size: 16.0,
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
