use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

/// Read the current selection. On KDE Plasma Wayland `ext-data-control` is
/// advertised, so `wl-paste --primary` reads the highlighted text copy-free.
pub fn read_primary() -> Result<String> {
    for (cmd, args) in [
        ("wl-paste", &["--primary", "--no-newline"][..]),
        ("wl-paste", &["--no-newline"][..]),
        ("xclip", &["-selection", "primary", "-o"][..]),
    ] {
        if let Ok(s) = run(cmd, args) {
            if !s.trim().is_empty() {
                return Ok(s);
            }
        }
    }
    bail!("no text selected (PRIMARY selection and clipboard are empty)")
}

/// Show a desktop notification (reliable on Wayland/KDE from a background daemon,
/// unlike trying to surface our own window).
pub fn notify(summary: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "AI Translate", "-i", "accessories-dictionary", "-t", "12000"])
        .arg(summary)
        .arg(body)
        .status();
}

pub fn set_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .context("spawning wl-copy")?;
    child
        .stdin
        .as_mut()
        .context("wl-copy stdin")?
        .write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

/// Capture a screen region (KDE Spectacle picker), OCR it with tesseract.
pub fn ocr_region(langs: &str) -> Result<String> {
    let tmp = std::env::temp_dir().join("ai-translate-ocr.png");
    let _ = std::fs::remove_file(&tmp);

    // -r region, -b background (no main window), -n no notification, -o output
    let status = Command::new("spectacle")
        .args(["-r", "-b", "-n", "-o"])
        .arg(&tmp)
        .status()
        .context("launching spectacle — is it installed?")?;
    if !status.success() {
        bail!("region capture was cancelled");
    }
    if !tmp.exists() {
        bail!("no region was captured");
    }

    let out = Command::new("tesseract")
        .arg(&tmp)
        .arg("stdout")
        .args(["-l", langs])
        .output()
        .context("running tesseract — is it installed?")?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        bail!("tesseract failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        bail!("OCR found no text in the captured region (try a clearer area or add a language pack)");
    }
    Ok(text)
}

fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("running {cmd}"))?;
    if !out.status.success() {
        return Err(anyhow!("{cmd} exited with {:?}", out.status.code()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
