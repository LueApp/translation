use crate::config::Config;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::time::Duration;

/// Shared HTTP agent for every backend. Honors an optional proxy (needed to
/// reach providers blocked on this network, e.g. Google behind the GFW) and
/// bounds connect/total time so an unreachable endpoint fails fast instead of
/// hanging. `http_status_as_error(false)` lets each backend read 4xx/5xx
/// bodies and surface the server's own message instead of a bare status code.
fn http_agent(cfg: &Config) -> Result<ureq::Agent> {
    let mut builder = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_global(Some(Duration::from_secs(40)))
        .http_status_as_error(false);
    let proxy = cfg.proxy_url.trim();
    if !proxy.is_empty() {
        let p = ureq::Proxy::new(proxy).with_context(|| {
            format!("invalid proxy_url '{proxy}' (use http://host:port or socks5://host:port)")
        })?;
        builder = builder.proxy(Some(p));
    }
    Ok(builder.build().into())
}

/// Translate `text` using the provider configured in `cfg`.
pub fn translate(cfg: &Config, text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    match cfg.provider.as_str() {
        "mymemory" => mymemory(cfg, text),
        // AI is the quality backend; until a key is set, fall back to the free
        // engine so the tool always works.
        "ai" if cfg.ai_key.trim().is_empty() => mymemory(cfg, text),
        "ai" => ai_translate(cfg, text),
        "libre" => libretranslate(cfg, text),
        "google" => {
            let agent = http_agent(cfg)?;
            let (src, tgt) = resolve_langs(cfg, text);
            google_free(&agent, &src, &tgt, text)
        }
        other => bail!("unknown provider '{}'", other),
    }
}

// ---------- MyMemory: free, no key (reachable in CN) ----------

fn mymemory(cfg: &Config, text: &str) -> Result<String> {
    let agent = http_agent(cfg)?;
    let (src, tgt) = resolve_langs(cfg, text);
    let langpair = format!("{src}|{tgt}");
    // MyMemory caps each request at 500 bytes — translate long text in chunks.
    translate_chunked(text, 480, |chunk| mymemory_one(&agent, &langpair, chunk))
}

fn mymemory_one(agent: &ureq::Agent, langpair: &str, text: &str) -> Result<String> {
    let mut res = agent
        .get("https://api.mymemory.translated.net/get")
        .query("q", text)
        .query("langpair", langpair)
        .call()
        .context("MyMemory request failed (check network)")?;
    let v: Value = res.body_mut().read_json().context("reading MyMemory response")?;
    let status = v.get("responseStatus").and_then(value_as_i64).unwrap_or(0);
    let translated = v
        .pointer("/responseData/translatedText")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if status != 200 || translated.is_empty() {
        let detail = v
            .get("responseDetails")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown error");
        bail!("MyMemory error ({status}): {detail}");
    }
    Ok(translated.to_string())
}

// ---------- AI: OpenAI-compatible chat (DeepSeek/Kimi/GLM/Qwen/Doubao/OpenAI) ----------

fn ai_translate(cfg: &Config, text: &str) -> Result<String> {
    if cfg.ai_key.trim().is_empty() {
        bail!("no AI key set — put your key in ai_key (see `ai-translate config-path`)");
    }
    let (src, tgt) = resolve_langs(cfg, text);
    let src_name = lang_name(&src);
    let tgt_name = lang_name(&tgt);
    let system = format!(
        "You are a professional translator. Translate the user's text from {src_name} into {tgt_name}. \
         Preserve meaning, tone, terminology and formatting. Output ONLY the translation — no quotes, \
         no explanations, no notes, no romanization."
    );
    let url = format!("{}/chat/completions", cfg.ai_base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": cfg.ai_model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": text},
        ],
        "temperature": 0.3,
        "stream": false,
    });
    let auth = format!("Bearer {}", cfg.ai_key.trim());
    let agent = http_agent(cfg)?;
    let mut res = agent
        .post(&url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .send_json(&body)
        .with_context(|| format!("AI request to {url} failed"))?;
    let v: Value = res.body_mut().read_json().context("reading AI response")?;
    match v.pointer("/choices/0/message/content").and_then(|x| x.as_str()) {
        Some(content) => Ok(content.trim().to_string()),
        // Surface the provider's own error (bad key, unknown model, …) clearly.
        None => match v.pointer("/error/message").and_then(|x| x.as_str()) {
            Some(msg) => bail!("AI provider error: {msg}"),
            None => bail!("unexpected AI response: {v}"),
        },
    }
}

// ---------- LibreTranslate (self-hostable; public instance needs a key) ----------

fn libretranslate(cfg: &Config, text: &str) -> Result<String> {
    let url = format!("{}/translate", cfg.libre_url.trim_end_matches('/'));
    let (src, tgt) = resolve_langs(cfg, text);
    let mut body = serde_json::json!({
        "q": text,
        "source": base(&src),
        "target": base(&tgt),
        "format": "text",
    });
    if !cfg.libre_key.is_empty() {
        body["api_key"] = Value::String(cfg.libre_key.clone());
    }
    let agent = http_agent(cfg)?;
    let mut res = agent
        .post(&url)
        .send_json(&body)
        .context("LibreTranslate request failed")?;
    let v: Value = res
        .body_mut()
        .read_json()
        .context("reading LibreTranslate response")?;
    v.get("translatedText")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("LibreTranslate error: {v}"))
}

// ---------- Google unofficial (free, blocked behind the GFW) ----------

fn google_free(agent: &ureq::Agent, sl: &str, tl: &str, text: &str) -> Result<String> {
    // Keep each GET request's URL within bounds.
    translate_chunked(text, 1500, |chunk| google_one(agent, sl, tl, chunk))
}

fn google_one(agent: &ureq::Agent, sl: &str, tl: &str, text: &str) -> Result<String> {
    let mut res = agent
        .get("https://translate.googleapis.com/translate_a/single")
        .query("client", "gtx")
        .query("sl", sl)
        .query("tl", tl)
        .query("dt", "t")
        .query("q", text)
        .call()
        .context(
            "Google translate request failed — it's blocked on this network. \
             Set `proxy_url` in config (e.g. http://127.0.0.1:7890) to route through a VPN/proxy.",
        )?;
    let body = res.body_mut().read_to_string().context("reading Google response")?;
    let v: Value = serde_json::from_str(&body).context("parsing Google JSON")?;
    let segments = v
        .get(0)
        .and_then(|x| x.as_array())
        .ok_or_else(|| anyhow!("unexpected Google response shape"))?;
    let mut out = String::new();
    for seg in segments {
        if let Some(s) = seg.get(0).and_then(|x| x.as_str()) {
            out.push_str(s);
        }
    }
    Ok(out)
}

// ---------- language helpers ----------

/// Resolve (source, target) for a request, handling "auto" detection and the
/// bilingual toggle: if the detected source already equals the target, flip the
/// target (zh⇄en) so "translate my selection" always does something useful.
fn resolve_langs(cfg: &Config, text: &str) -> (String, String) {
    let mut tgt = if cfg.target_lang.trim().is_empty() {
        "en".to_string()
    } else {
        cfg.target_lang.clone()
    };
    let src = if cfg.source_lang.trim().is_empty()
        || cfg.source_lang.trim().eq_ignore_ascii_case("auto")
    {
        detect_source(text).to_string()
    } else {
        cfg.source_lang.clone()
    };
    if base(&src) == base(&tgt) {
        tgt = if base(&src) == "en" {
            "zh-CN".to_string()
        } else {
            "en".to_string()
        };
    }
    (src, tgt)
}

fn base(code: &str) -> &str {
    code.split('-').next().unwrap_or(code)
}

/// Cheap script-based language detection for the common CJK/Latin cases.
fn detect_source(text: &str) -> &'static str {
    let mut cjk = 0usize;
    let mut hira_kata = 0usize;
    let mut hangul = 0usize;
    let mut latin = 0usize;
    for c in text.chars() {
        let u = c as u32;
        if (0x4E00..=0x9FFF).contains(&u) {
            cjk += 1;
        } else if (0x3040..=0x30FF).contains(&u) {
            hira_kata += 1;
        } else if (0xAC00..=0xD7AF).contains(&u) {
            hangul += 1;
        } else if c.is_ascii_alphabetic() {
            latin += 1;
        }
    }
    if hangul > 0 && hangul >= cjk {
        "ko"
    } else if hira_kata > 0 {
        "ja"
    } else if cjk > 0 && cjk * 3 >= latin {
        "zh-CN"
    } else {
        "en"
    }
}

fn lang_name(code: &str) -> String {
    match base(code) {
        "zh" => "Chinese (Simplified)".into(),
        "en" => "English".into(),
        "ja" => "Japanese".into(),
        "ko" => "Korean".into(),
        "fr" => "French".into(),
        "de" => "German".into(),
        "es" => "Spanish".into(),
        "ru" => "Russian".into(),
        "auto" => "the source language".into(),
        other => other.to_string(),
    }
}

fn value_as_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

// ---------- chunking (for length-limited backends like MyMemory / Google) ----------

/// Translate `text` by splitting it into byte-bounded chunks and joining the
/// per-chunk translations. Single-request backends pass the whole text.
fn translate_chunked(
    text: &str,
    max_bytes: usize,
    f: impl Fn(&str) -> Result<String>,
) -> Result<String> {
    let chunks = chunk_text(text, max_bytes);
    if chunks.len() <= 1 {
        return f(text);
    }
    let mut out = String::new();
    for chunk in chunks {
        if chunk.trim().is_empty() {
            out.push_str(&chunk);
            continue;
        }
        out.push_str(f(&chunk)?.trim_end());
        if !out.ends_with(['\n', ' ']) {
            out.push(' ');
        }
    }
    Ok(out.trim_end().to_string())
}

/// Split text into chunks each ≤ max_bytes, preferring sentence / line breaks.
fn chunk_text(text: &str, max_bytes: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for unit in split_units(text) {
        if unit.len() > max_bytes {
            if !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
            }
            chunks.extend(hard_split(&unit, max_bytes));
        } else if cur.len() + unit.len() > max_bytes {
            chunks.push(std::mem::take(&mut cur));
            cur = unit;
        } else {
            cur.push_str(&unit);
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Break text after sentence-ending punctuation and newlines (delimiters kept).
fn split_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(
            c,
            '\n' | '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；' | '…'
        ) {
            units.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        units.push(cur);
    }
    units
}

/// Last-resort split of an oversized unit at char boundaries.
fn hard_split(s: &str, max_bytes: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if !cur.is_empty() && cur.len() + c.len_utf8() > max_bytes {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
