use anyhow::{Context, Result, anyhow, bail};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WL_COPY_EXIT_TIMEOUT: Duration = Duration::from_secs(2);

/// Read the live PRIMARY selection when the focused app publishes one.
pub fn read_primary() -> Result<String> {
    read_primary_with(run)
}

fn read_primary_with(mut read: impl FnMut(&str, &[&str]) -> Result<String>) -> Result<String> {
    for (cmd, args) in [
        ("wl-paste", &["--primary", "--no-newline"][..]),
        ("xclip", &["-selection", "primary", "-o"][..]),
    ] {
        if let Ok(text) = read(cmd, args) {
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
    }
    bail!("No selected text available. Copy it with Ctrl+C and use clipboard translation.")
}

/// Read text the user has already copied to the regular clipboard.
pub fn read_clipboard() -> Result<String> {
    read_clipboard_with(run)
}

fn read_clipboard_with(mut read: impl FnMut(&str, &[&str]) -> Result<String>) -> Result<String> {
    for (cmd, args) in [
        ("wl-paste", &["--no-newline"][..]),
        ("xclip", &["-selection", "clipboard", "-o"][..]),
    ] {
        if let Ok(s) = read(cmd, args) {
            if !s.trim().is_empty() {
                return Ok(s);
            }
        }
    }
    bail!("Clipboard has no text. Press Ctrl+C in the source app, then try again.")
}

/// Show a desktop notification (reliable on Wayland/KDE from a background daemon,
/// unlike trying to surface our own window).
pub fn notify(summary: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args([
            "-a",
            "AI Translate",
            "-i",
            "accessories-dictionary",
            "-t",
            "12000",
        ])
        .arg(summary)
        .arg(body)
        .status();
}

pub fn set_clipboard(text: &str) -> Result<()> {
    // Plasma's Klipper speaks KWin's native ext-data-control protocol. Prefer
    // it over wl-copy: wl-clipboard 2.2 only understands the older zwlr
    // data-control protocol and can otherwise hang while waiting for its
    // focus-dependent fallback surface to be activated.
    if set_clipboard_via_klipper(text) {
        return Ok(());
    }

    let child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .context("spawning wl-copy")?;

    write_to_child(child, text.as_bytes(), "wl-copy", WL_COPY_EXIT_TIMEOUT)
}

fn set_clipboard_via_klipper(text: &str) -> bool {
    Command::new("busctl")
        .args([
            "--user",
            "--timeout=2s",
            "call",
            "org.kde.klipper",
            "/klipper",
            "org.kde.klipper.klipper",
            "setClipboardContents",
            "s",
        ])
        .arg(text)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn write_to_child(
    mut child: Child,
    input: &[u8],
    command: &str,
    exit_timeout: Duration,
) -> Result<()> {
    // wl-copy reads until EOF before it offers the clipboard selection. Taking
    // stdin lets us close the pipe before wait(), otherwise each side waits for
    // the other indefinitely and the translation UI appears to be stuck.
    {
        let mut stdin = child
            .stdin
            .take()
            .with_context(|| format!("{command} stdin"))?;
        stdin
            .write_all(input)
            .with_context(|| format!("writing to {command}"))?;
    }

    let deadline = Instant::now() + exit_timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("waiting for {command}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{command} did not acquire the Wayland clipboard within {exit_timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        bail!("{command} exited with {:?}", status.code());
    }
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
        bail!(
            "OCR found no text in the captured region (try a clearer area or add a language pack)"
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_capture_does_not_read_regular_clipboard() {
        let mut calls = Vec::new();
        let text = read_primary_with(|cmd, args| {
            calls.push((
                cmd.to_string(),
                args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            ));
            if cmd == "xclip" {
                Ok("selected text".into())
            } else {
                Err(anyhow!("no Wayland PRIMARY"))
            }
        })
        .unwrap();
        assert_eq!(text, "selected text");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, ["--primary", "--no-newline"]);
        assert_eq!(calls[1].1, ["-selection", "primary", "-o"]);
    }

    #[test]
    fn reads_regular_wayland_clipboard_without_primary() {
        let mut calls = Vec::new();
        let text = read_clipboard_with(|cmd, args| {
            calls.push((
                cmd.to_string(),
                args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            ));
            Ok("copied text".into())
        })
        .unwrap();
        assert_eq!(text, "copied text");
        assert_eq!(calls, [("wl-paste".into(), vec!["--no-newline".into()])]);
    }

    #[test]
    fn falls_back_to_x11_clipboard_without_primary() {
        let mut calls = Vec::new();
        let text = read_clipboard_with(|cmd, args| {
            calls.push((
                cmd.to_string(),
                args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            ));
            if cmd == "xclip" {
                Ok("copied in XWayland".into())
            } else {
                Err(anyhow!("no Wayland clipboard"))
            }
        })
        .unwrap();
        assert_eq!(text, "copied in XWayland");
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[1],
            (
                "xclip".into(),
                vec!["-selection".into(), "clipboard".into(), "-o".into()]
            )
        );
    }

    #[test]
    fn closes_child_stdin_before_waiting() {
        // `timeout` makes this regression bounded: if stdin remains open, cat
        // exits with 124 after one second and the assertion fails instead of
        // hanging the test suite.
        let child = Command::new("timeout")
            .args(["1", "cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();

        write_to_child(
            child,
            b"translated text",
            "test command",
            Duration::from_secs(1),
        )
        .unwrap();
    }

    #[test]
    fn kills_child_that_does_not_exit() {
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();

        let started = Instant::now();
        let error = write_to_child(
            child,
            b"translated text",
            "test command",
            Duration::from_millis(50),
        )
        .unwrap_err();

        assert!(error.to_string().contains("did not acquire"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
