use anyhow::{Context, Result, bail};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use zbus::{Proxy, connection};

const REPORT_PATH: &str = "/io/github/lue/AiTranslate/Focus";
const REPORT_INTERFACE: &str = "io.github.lue.AiTranslate.Focus";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureRoute {
    Primary,
    Clipboard,
}

struct WindowInfo {
    class_name: String,
    title: String,
}

struct WindowReport {
    sender: Option<oneshot::Sender<WindowInfo>>,
}

#[zbus::interface(name = "io.github.lue.AiTranslate.Focus")]
impl WindowReport {
    #[zbus(name = "Report")]
    fn report(&mut self, class_name: String, title: String) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(WindowInfo { class_name, title });
        }
    }
}

/// KWin is the only reliable source of the active native Wayland window.
/// A one-shot script reports its class and caption to this process over D-Bus.
pub fn capture_route() -> Result<CaptureRoute> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if !desktop.to_ascii_uppercase().contains("KDE") {
        return Ok(CaptureRoute::Primary);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let window = runtime.block_on(active_window())?;
    Ok(route_for(&window.class_name, &window.title))
}

fn route_for(class_name: &str, title: &str) -> CaptureRoute {
    let class_name = class_name.to_ascii_lowercase();
    let title = title.to_ascii_lowercase();
    if class_name.contains("wechat")
        || class_name.contains("codex")
        || class_name.contains("google-chrome")
        || class_name.contains("chromium")
        || title.contains("codex")
    {
        CaptureRoute::Clipboard
    } else {
        CaptureRoute::Primary
    }
}

async fn active_window() -> Result<WindowInfo> {
    let (sender, receiver) = oneshot::channel();
    let connection = connection::Builder::session()?
        .serve_at(
            REPORT_PATH,
            WindowReport {
                sender: Some(sender),
            },
        )?
        .build()
        .await?;
    let address = connection
        .unique_name()
        .context("no unique session-bus name")?
        .to_string();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let script_name = format!("ait-capture-{}-{nonce}", std::process::id());
    let path = std::env::temp_dir().join(format!("{script_name}.js"));
    let script = format!(
        "const w = workspace.activeWindow;\nconst cls = w ? String(w.resourceClass || '') : '';\nconst title = w ? String(w.caption || '') : '';\ncallDBus('{address}', '{REPORT_PATH}', '{REPORT_INTERFACE}', 'Report', cls, title);\n"
    );
    std::fs::write(&path, script)?;

    let result = run_script(&connection, &path, &script_name, receiver).await;
    let _ = std::fs::remove_file(&path);
    result
}

async fn run_script(
    connection: &zbus::Connection,
    path: &std::path::Path,
    script_name: &str,
    receiver: oneshot::Receiver<WindowInfo>,
) -> Result<WindowInfo> {
    let scripting = Proxy::new(
        connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )
    .await?;
    let script_id: i32 = scripting
        .call(
            "loadScript",
            &(path.to_string_lossy().to_string(), script_name),
        )
        .await?;
    if script_id < 0 {
        bail!("KWin could not load active-window probe");
    }
    let result = async {
        let script_path = format!("/Scripting/Script{script_id}");
        let script = Proxy::new(
            connection,
            "org.kde.KWin",
            script_path.as_str(),
            "org.kde.kwin.Script",
        )
        .await?;
        let _: () = script.call("run", &()).await?;
        let window = tokio::time::timeout(Duration::from_secs(2), receiver)
            .await
            .context("KWin active-window probe timed out")??;
        let _: Result<(), _> = script.call("stop", &()).await;
        Ok(window)
    }
    .await;
    let _: Result<(), _> = scripting.call("unloadScript", &(script_name,)).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_special_apps_to_clipboard_and_others_to_primary() {
        assert_eq!(route_for("wechat", "微信"), CaptureRoute::Clipboard);
        assert_eq!(route_for("WeChatAppEx", ""), CaptureRoute::Clipboard);
        assert_eq!(
            route_for("google-chrome", "Project workspace"),
            CaptureRoute::Clipboard
        );
        assert_eq!(route_for("codex", ""), CaptureRoute::Clipboard);
        assert_eq!(route_for("org.kde.konsole", "Codex"), CaptureRoute::Clipboard);
        assert_eq!(route_for("org.kde.konsole", "Shell"), CaptureRoute::Primary);
        assert_eq!(route_for("firefox", "Documentation"), CaptureRoute::Primary);
    }
}
