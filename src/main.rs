mod capture;
mod config;
mod daemon;
mod kde_shortcuts;
mod translate;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;

#[derive(Parser)]
#[command(name = "ai-translate", version, about = "Native KDE/Wayland AI translator")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open an empty popup to type text (default)
    Popup,
    /// Translate the current selection (PRIMARY), copy-free on KDE Wayland
    Selection,
    /// Capture a screen region, OCR it, and translate
    Ocr,
    /// Translate a string and print to stdout (no GUI)
    Text { text: Vec<String> },
    /// Run the background daemon that registers global hotkeys
    Daemon,
    /// Print the path of the config file
    ConfigPath,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    match cli.cmd.unwrap_or(Cmd::Popup) {
        Cmd::Popup => run_gui(cfg, String::new(), false, false),
        Cmd::Selection => {
            let text = capture::read_primary().unwrap_or_default();
            run_gui(cfg, text, true, true)
        }
        Cmd::Ocr => {
            let langs = cfg.ocr_langs.clone();
            match capture::ocr_region(&langs) {
                Ok(text) => run_gui(cfg, text, true, true),
                Err(e) => {
                    capture::notify("AI Translate — OCR", &e.to_string());
                    Ok(())
                }
            }
        }
        Cmd::Text { text } => {
            let translation = translate::translate_with_warning(&cfg, &text.join(" "))?;
            if let Some(warning) = translation.warning {
                eprintln!("Warning: {warning}");
            }
            println!("{}", translation.text);
            Ok(())
        }
        Cmd::Daemon => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(daemon::run())
        }
        Cmd::ConfigPath => {
            println!("{}", Config::path()?.display());
            Ok(())
        }
    }
}

/// Ask KWin (via its scripting D-Bus) to move our window to the mouse cursor,
/// clamped to the cursor's monitor. This is the only reliable way to place a
/// window at the cursor on KDE Wayland (clients can't self-position; logical
/// coords via `workspace.cursorPos`).
fn reposition_at_cursor() {
    // NOTE: KWin's JS engine has no `Qt.rect`; mutate the QRect and reassign.
    const JS: &str = r#"
const p = workspace.cursorPos;
const list = (workspace.windowList ? workspace.windowList() : workspace.clientList());
let outs = [];
try { outs = workspace.outputs || workspace.screens || []; } catch (e) {}
let scr = null;
for (let i = 0; i < outs.length; i++) { const g = outs[i].geometry;
  if (p.x >= g.x && p.x < g.x + g.width && p.y >= g.y && p.y < g.y + g.height) { scr = g; break; } }
for (let i = 0; i < list.length; i++) { const w = list[i];
  if (w.resourceClass == "ai-translate") {
    let g = w.frameGeometry;
    let x = p.x - 24, y = p.y - 12;
    if (scr) {
      if (x + g.width  > scr.x + scr.width)  x = scr.x + scr.width  - g.width;
      if (y + g.height > scr.y + scr.height) y = scr.y + scr.height - g.height;
      if (x < scr.x) x = scr.x;
      if (y < scr.y) y = scr.y;
    }
    g.x = x; g.y = y; w.frameGeometry = g;
  }
}
"#;
    let path = std::env::temp_dir().join("ait-move.js");
    if std::fs::write(&path, JS).is_err() {
        return;
    }
    let p = path.to_string_lossy().to_string();
    let call = |args: &[&str]| {
        let _ = std::process::Command::new("qdbus6").args(args).status();
    };
    call(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", "ait-move"]);
    call(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", &p, "ait-move"]);
    call(&["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"]);
}

fn run_gui(cfg: Config, initial: String, auto: bool, auto_copy: bool) -> Result<()> {
    // Move the window to the cursor shortly after it maps (KWin needs the window
    // to exist). Two attempts cover slow first-paint.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(350));
        reposition_at_cursor();
        std::thread::sleep(std::time::Duration::from_millis(400));
        reposition_at_cursor();
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([580.0, 560.0])
            .with_min_inner_size([360.0, 300.0])
            .with_title("AI Translate")
            .with_app_id("ai-translate"),
        // Surfacing (keep-above), focus, and Under-Mouse placement are handled by
        // the KWin window rule for wmclass "ai-translate" — setting them here too
        // can conflict with KWin's placement.
        ..Default::default()
    };
    eframe::run_native(
        "AI Translate",
        options,
        Box::new(move |cc| Ok(Box::new(ui::TranslatorApp::new(cc, cfg, initial, auto, auto_copy)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;
    Ok(())
}
