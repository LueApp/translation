use crate::config::Config;
use anyhow::{Context, Result, anyhow, bail};
use ashpd::desktop::{
    PersistMode,
    remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions},
};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as XfixesConnectionExt, SelectionEventMask};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as XprotoConnectionExt};
use x11rb::rust_connection::RustConnection;

const WL_COPY_EXIT_TIMEOUT: Duration = Duration::from_secs(2);
const SELECTION_COPY_TIMEOUT: Duration = Duration::from_millis(1200);

/// Ask the focused app to copy its selection. Input fields in Chrome and WeChat
/// often do not publish PRIMARY, so reading it can return a previous selection.
pub fn read_selection() -> Result<String> {
    let x11 = focused_window_is_x11();
    let clipboard = if x11 {
        read_x11_clipboard
    } else {
        read_wayland_clipboard
    };
    let previous = clipboard().unwrap_or_default();
    let mut watcher = ClipboardWatcher::new().ok();

    // Let the triggering global shortcut's modifier key go before Ctrl+C.
    std::thread::sleep(Duration::from_millis(100));
    if x11 {
        let status = Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+c"])
            .status()
            .context("sending Ctrl+C to the focused X11 app")?;
        if !status.success() {
            bail!("xdotool could not copy from the focused app");
        }
    } else {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("starting Wayland selection capture")?;
        runtime.block_on(copy_via_remote_desktop())?;
    }

    wait_for_fresh_clipboard(
        &previous,
        || watcher.as_mut().is_some_and(|watcher| watcher.changed()),
        clipboard,
    )
}

fn focused_window_is_x11() -> bool {
    let Ok((connection, screen)) = x11rb::connect(None) else {
        return false;
    };
    let root = connection.setup().roots[screen].root;
    let Some(top_level) = connection
        .get_input_focus()
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| top_level_window(&connection, reply.focus, root))
    else {
        return false;
    };
    connection
        .get_property(
            false,
            top_level,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            0,
            256,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| !reply.value.is_empty())
}

fn read_x11_clipboard() -> Result<String> {
    run("xclip", &["-selection", "clipboard", "-o"])
}

fn top_level_window(connection: &RustConnection, mut window: u32, root: u32) -> Option<u32> {
    for _ in 0..64 {
        if window == 0 || window == root {
            return None;
        }
        let parent = connection.query_tree(window).ok()?.reply().ok()?.parent;
        if parent == root {
            return Some(window);
        }
        window = parent;
    }
    None
}

fn read_wayland_clipboard() -> Result<String> {
    run("wl-paste", &["--no-newline"])
}

fn wait_for_fresh_clipboard(
    previous: &str,
    mut changed: impl FnMut() -> bool,
    mut read: impl FnMut() -> Result<String>,
) -> Result<String> {
    let deadline = Instant::now() + SELECTION_COPY_TIMEOUT;
    let mut saw_change = false;
    loop {
        saw_change |= changed();
        if let Ok(text) = read() {
            if !text.trim().is_empty() && (saw_change || text != previous) {
                return Ok(text);
            }
        }
        if Instant::now() >= deadline {
            bail!("Could not copy selected text. Keep it highlighted and try again.");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// XFixes reports a new CLIPBOARD owner even when the copied text equals the
/// previous clipboard text (common with KDE's selection/clipboard sync).
struct ClipboardWatcher {
    connection: RustConnection,
    clipboard_atom: u32,
}

impl ClipboardWatcher {
    fn new() -> Result<Self> {
        let (connection, screen) = x11rb::connect(None)?;
        let clipboard_atom = connection.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
        connection.xfixes_query_version(5, 0)?.reply()?;
        connection
            .xfixes_select_selection_input(
                connection.setup().roots[screen].root,
                clipboard_atom,
                SelectionEventMask::SET_SELECTION_OWNER,
            )?
            .check()?;
        connection.flush()?;
        Ok(Self {
            connection,
            clipboard_atom,
        })
    }

    fn changed(&mut self) -> bool {
        while let Ok(Some(event)) = self.connection.poll_for_event() {
            if let Event::XfixesSelectionNotify(event) = event {
                if event.selection == self.clipboard_atom && event.owner != 0 {
                    return true;
                }
            }
        }
        false
    }
}

async fn copy_via_remote_desktop() -> Result<()> {
    let portal = RemoteDesktop::new()
        .await
        .context("Wayland keyboard-control portal is unavailable")?;
    let session = portal.create_session(Default::default()).await?;
    let token_path = Config::dir()?.join("remote-desktop-token");
    let token = std::fs::read_to_string(&token_path).ok();
    let options = SelectDevicesOptions::default()
        .set_devices(Some(DeviceType::Keyboard.into()))
        .set_persist_mode(PersistMode::ExplicitlyRevoked)
        .set_restore_token(token.as_deref().map(str::trim));
    portal.select_devices(&session, options).await?.response()?;
    let response = portal
        .start(&session, None, Default::default())
        .await?
        .response()?;
    if !response.devices().contains(DeviceType::Keyboard) {
        bail!("Keyboard access was not granted; copy and paste into the popup instead.");
    }
    if let Some(token) = response.restore_token() {
        save_remote_desktop_token(&token_path, token)?;
    }

    // The first request may show a permission dialog; allow the target app to
    // regain focus after it closes before injecting Ctrl+C.
    std::thread::sleep(Duration::from_millis(150));
    portal
        .notify_keyboard_keycode(&session, 29, KeyState::Pressed, Default::default())
        .await?;
    let press_c = portal
        .notify_keyboard_keycode(&session, 46, KeyState::Pressed, Default::default())
        .await;
    let release_c = portal
        .notify_keyboard_keycode(&session, 46, KeyState::Released, Default::default())
        .await;
    let release_ctrl = portal
        .notify_keyboard_keycode(&session, 29, KeyState::Released, Default::default())
        .await;
    press_c?;
    release_c?;
    release_ctrl?;
    std::thread::sleep(Duration::from_millis(100));
    Ok(())
}

fn save_remote_desktop_token(path: &std::path::Path, token: &str) -> Result<()> {
    let dir = path.parent().context("remote-desktop token directory")?;
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(token.as_bytes())?;
    Ok(())
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
    fn waits_for_a_fresh_copy() {
        let mut values = ["old", "", "selected text"].into_iter();
        let text = wait_for_fresh_clipboard(
            "old",
            || false,
            || Ok(values.next().unwrap_or("selected text").to_string()),
        )
        .unwrap();
        assert_eq!(text, "selected text");
    }

    #[test]
    fn accepts_same_text_after_a_copy_event() {
        let text =
            wait_for_fresh_clipboard("selected text", || true, || Ok("selected text".into()))
                .unwrap();
        assert_eq!(text, "selected text");
    }

    #[test]
    fn unchanged_clipboard_is_not_a_selection() {
        let error = wait_for_fresh_clipboard("old", || false, || Ok("old".into())).unwrap_err();
        assert!(error.to_string().contains("Could not copy selected text"));
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
