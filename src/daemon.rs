use anyhow::{Context, Result};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;

/// Long-running daemon: registers the global hotkeys and spawns the matching
/// action when one fires. On KDE we use the native KGlobalAccel API (lets us set
/// the active key with no consent dialog); elsewhere we use the XDG
/// GlobalShortcuts portal (GNOME 48+, wlroots, …).
pub async fn run() -> Result<()> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.to_uppercase().contains("KDE") {
        eprintln!("[daemon] KDE detected — using native KGlobalAccel shortcuts");
        return crate::kde_shortcuts::run().await;
    }
    eprintln!("[daemon] using XDG GlobalShortcuts portal");
    portal_run().await
}

/// XDG GlobalShortcuts portal path (GNOME 48+, KDE fallback, wlroots).
async fn portal_run() -> Result<()> {
    eprintln!("[daemon] connecting to GlobalShortcuts portal…");
    let gs = GlobalShortcuts::new()
        .await
        .context("GlobalShortcuts portal unavailable on this session")?;
    eprintln!("[daemon] portal ok (v{}); creating session…", gs.version());

    let session = gs
        .create_session(Default::default())
        .await
        .context("create_session failed")?;
    eprintln!("[daemon] session created; binding shortcuts…");

    let shortcuts = [
        NewShortcut::new("translate_selection", "Translate the selected text")
            .preferred_trigger("CTRL+ALT+S"),
        NewShortcut::new("translate_ocr", "Capture a screen region, OCR and translate")
            .preferred_trigger("CTRL+ALT+O"),
        NewShortcut::new("translate_popup", "Open the translate popup")
            .preferred_trigger("CTRL+ALT+T"),
    ];

    gs.bind_shortcuts(&session, &shortcuts, None, Default::default())
        .await
        .context("bind_shortcuts failed")?;

    eprintln!(
        "ai-translate daemon: shortcuts bound. Adjust keys in System Settings → Shortcuts. Listening…"
    );

    let exe =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("ai-translate"));

    let mut activated = gs
        .receive_activated()
        .await
        .context("receive_activated failed")?;

    while let Some(act) = activated.next().await {
        let action = match act.shortcut_id() {
            "translate_selection" => "selection",
            "translate_ocr" => "ocr",
            _ => "popup",
        };
        eprintln!("[daemon] {} -> {}", act.shortcut_id(), action);
        if let Err(e) = std::process::Command::new(&exe).arg(action).spawn() {
            eprintln!("[daemon] failed to spawn action '{action}': {e}");
        }
    }

    Ok(())
}
