use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use zbus::{Connection, Proxy};

const COMPONENT: &str = "io.github.lue.AiTranslate";
const COMPONENT_FRIENDLY: &str = "AI Translate";

// Qt keyboard modifier flags.
const SHIFT: i32 = 0x0200_0000;
const CTRL: i32 = 0x0400_0000;
const ALT: i32 = 0x0800_0000;
const META: i32 = 0x1000_0000;

fn cand(mods: i32, c: char, label: &'static str) -> (&'static str, i32) {
    (label, mods | (c as i32))
}

struct Sc {
    id: &'static str,
    friendly: &'static str,
    candidates: Vec<(&'static str, i32)>,
}

/// Native KDE global shortcuts via the KGlobalAccel D-Bus API.
///
/// zbus is used only for the one-shot registration (it lets us set the *active*
/// key with no consent dialog, unlike the GlobalShortcuts portal). We then keep
/// that connection alive (so KGlobalAccel keeps the component active) and watch
/// for presses by parsing `busctl monitor` in a blocking thread — this avoids
/// zbus's tokio-executor fragility on the long-lived signal path.
pub async fn run() -> Result<()> {
    let shortcuts = [
        Sc {
            id: "translate_selection",
            friendly: "Translate the selected text",
            candidates: vec![
                cand(META, 'E', "Meta+E"),
                cand(CTRL | ALT, 'E', "Ctrl+Alt+E"),
                cand(META, 'S', "Meta+S"),
                cand(CTRL | ALT, 'S', "Ctrl+Alt+S"),
            ],
        },
        Sc {
            id: "translate_ocr",
            friendly: "Capture a screen region, OCR and translate",
            candidates: vec![
                cand(META, 'R', "Meta+R"),
                cand(CTRL | ALT, 'R', "Ctrl+Alt+R"),
                cand(META | SHIFT, 'O', "Meta+Shift+O"),
                cand(CTRL | ALT, 'O', "Ctrl+Alt+O"),
            ],
        },
        Sc {
            id: "translate_popup",
            friendly: "Open the translate popup",
            candidates: vec![
                cand(META, 'T', "Meta+T"),
                cand(CTRL | ALT, 'G', "Ctrl+Alt+G"),
                cand(META | SHIFT, 'T', "Meta+Shift+T"),
                cand(CTRL | ALT, 'T', "Ctrl+Alt+T"),
            ],
        },
    ];

    let conn = Connection::session().await.context("connect to session bus")?;
    let kga = Proxy::new(
        &conn,
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel",
    )
    .await
    .context("KGlobalAccel proxy")?;

    for s in &shortcuts {
        let action_id = vec![
            COMPONENT.to_string(),
            s.id.to_string(),
            COMPONENT_FRIENDLY.to_string(),
            s.friendly.to_string(),
        ];
        let _: () = kga
            .call("doRegister", &(action_id.clone(),))
            .await
            .with_context(|| format!("doRegister {}", s.id))?;

        let mut chosen = None;
        for (label, code) in &s.candidates {
            let keys = vec![*code];
            // flags: IsDefault(1) | SetPresent(2) | NoAutoloading(4) = 7.
            // SetPresent is what actually ACTIVATES the KWin grab; without it the
            // key is only reserved (invokeShortcut works but real presses don't).
            let assigned: Vec<i32> = kga
                .call("setShortcut", &(action_id.clone(), keys.clone(), 7u32))
                .await
                .unwrap_or_default();
            if assigned.first().copied().unwrap_or(0) == *code {
                chosen = Some(*label);
                break;
            }
        }
        match chosen {
            Some(label) => eprintln!("[kde] {} -> {}", s.id, label),
            None => eprintln!("[kde] {} -> NO FREE KEY (all candidates taken)", s.id),
        }
    }

    // Keep the D-Bus connection alive for the daemon's whole lifetime.
    // KGlobalAccel only holds the actual KWin key grab while the registering
    // owner stays connected — drop it and *real* key presses stop reaching us
    // (invokeShortcut still works, which is what misled earlier testing).
    // Presses themselves are caught by the busctl monitor (avoids zbus's
    // tokio-executor signal-path quirks); this connection just holds the grab.
    eprintln!("[kde] listening for global shortcut presses…");
    let _keepalive = (conn, kga);
    tokio::task::spawn_blocking(listen_loop)
        .await
        .context("listener thread")?
}

/// Parse `busctl monitor` output for `globalShortcutPressed` signals and spawn
/// the matching action. Pure std (no zbus on the hot path).
fn listen_loop() -> Result<()> {
    let mut child = Command::new("busctl")
        .args([
            "--user",
            "monitor",
            "--match",
            "type='signal',interface='org.kde.kglobalaccel.Component',member='globalShortcutPressed'",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn busctl monitor")?;

    let stdout = child.stdout.take().context("busctl stdout")?;
    let exe =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("ai-translate"));

    let mut capturing = false;
    let mut strings: Vec<String> = Vec::new();

    for line in BufReader::new(stdout).lines() {
        let line = line.context("read busctl output")?;
        if line.contains("globalShortcutPressed") {
            capturing = true;
            strings.clear();
            continue;
        }
        if !capturing {
            continue;
        }
        if line.contains("STRING") {
            if let Some(v) = extract_quoted(&line) {
                strings.push(v);
            }
        }
        // After component + action id are seen, dispatch.
        if strings.len() >= 2 {
            capturing = false;
            let component = &strings[0];
            let action_id = &strings[1];
            if component != COMPONENT {
                continue;
            }
            let action = match action_id.as_str() {
                "translate_selection" => "selection",
                "translate_ocr" => "ocr",
                _ => "popup",
            };
            eprintln!("[kde] pressed {action_id} -> spawning {action}");
            match Command::new(&exe).arg(action).spawn() {
                // Reap on a detached thread so closed windows don't become zombies.
                Ok(mut c) => {
                    std::thread::spawn(move || {
                        let _ = c.wait();
                    });
                }
                Err(e) => eprintln!("[kde] failed to spawn {action}: {e}"),
            }
        }
    }

    let _ = child.wait();
    anyhow::bail!("busctl monitor exited; restarting daemon");
}

fn extract_quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
