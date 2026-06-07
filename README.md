# AI Translate

A native, lightweight translation tool for Ubuntu/KDE — translate the **selection**,
a **screen region (OCR)**, or **typed text**, via a global hotkey, with a free
default engine and optional AI-model backends for much better quality.

Built in Rust (egui GUI — no webkit), tested on **Ubuntu 26.04 / KDE Plasma 6 (Wayland)**.

## Triggers

| Action | Hotkey | What it does |
|---|---|---|
| Translate selection | **Meta+S** | Reads the highlighted text (copy-free on KDE Wayland), translates it, shows the result in a window **at the mouse cursor**, and **copies it to the clipboard** |
| Capture & OCR | **Ctrl+Alt+R** | Drag a screen region → Tesseract OCR → translate → result window at cursor + clipboard |
| Popup | **Meta+Shift+T** | Opens a type/paste window at the cursor |

**Window placement on Wayland:** a background daemon's window can't self-position
or auto-raise on Wayland. Two KDE mechanisms make it work: a **KWin window rule**
(`wmclass=ai-translate` → keep-above + no focus-stealing) so it surfaces on top,
and a tiny **KWin script** (driven over D-Bus on open) that moves the window to
`workspace.cursorPos`, clamped to the cursor's monitor. The translation is also
always copied to the clipboard as a fallback.

The window rule lives in `~/.config/kwinrulesrc` (added automatically). If you
ever want the window elsewhere, change/remove that rule in System Settings →
Window Management → Window Rules ("AI Translate at cursor").

Hotkeys are registered with KDE's KGlobalAccel and can be changed in
**System Settings → Shortcuts** (component "AI Translate"). They were chosen to
avoid clashing with existing shortcuts (e.g. Crow Translate).

You can also launch from the app menu ("AI Translate"), and right-click the menu
entry for "Translate Selection" / "Capture & Translate (OCR)".

## Translation backends

Configured in `~/.config/ai-translate/config.toml` (`provider = …`):

- `mymemory` — **default**, free, no API key. Decent, but literal at times.
- `ai` — **OpenAI-compatible** chat API (your key). Best quality. Works with
  domestic LLMs reachable in CN: DeepSeek, Moonshot/Kimi, Zhipu/GLM, Qwen
  (DashScope compatible-mode), Doubao/Volcano — and OpenAI itself.
- `libre` — LibreTranslate. Defaults to the free, keyless mirror
  `https://translate.disroot.org` (point `libre_url` at your own instance if you
  self-host; `libretranslate.com` itself now requires an API key).
- `google` — free Google endpoint. **Blocked behind the GFW** — only works if you
  set `proxy_url` (see below) to route through a VPN/proxy.

### Proxy (for blocked endpoints)

To reach providers blocked on your network (e.g. Google), set a proxy that all
backends route through. Three ways, in precedence order:

1. **In-app** — click the **⚙** button in the popup, fill in **Proxy URL**, and
   press **Save** (writes `config.toml`). Same panel also sets your AI key,
   AI endpoint/model, and LibreTranslate URL — no file editing needed.
2. **Config file** — set `proxy_url` in `~/.config/ai-translate/config.toml`:
   ```toml
   proxy_url = "http://127.0.0.1:7890"     # or socks5://127.0.0.1:7891
   ```
3. **Environment** — if `proxy_url` is empty, `HTTP_PROXY` / `HTTPS_PROXY` /
   `ALL_PROXY` (and `NO_PROXY`) are honored automatically. Note: the hotkey
   daemon (a systemd user service) won't see vars you only `export` in a shell,
   so prefer #1 or #2 for the global-hotkey triggers.

Leave `proxy_url` empty (`""`) for a direct connection.

### Settings panel

The **⚙** button in the popup opens an in-app editor for everything that isn't a
per-translation choice — proxy, AI key/endpoint/model, LibreTranslate URL/key.
Edits apply to the current session immediately; **Save** persists them to
`~/.config/ai-translate/config.toml` for future launches.

### Enable AI (recommended)

Edit `~/.config/ai-translate/config.toml`:

```toml
provider = "ai"
ai_base_url = "https://api.deepseek.com/v1"   # or your provider's base
ai_model    = "deepseek-chat"                  # provider's model id
ai_key      = "sk-..."                          # your key
```

Common presets (base_url / model):

| Provider | ai_base_url | ai_model |
|---|---|---|
| DeepSeek | `https://api.deepseek.com/v1` | `deepseek-chat` |
| Moonshot (Kimi) | `https://api.moonshot.cn/v1` | `moonshot-v1-8k` |
| Zhipu (GLM) | `https://open.bigmodel.cn/api/paas/v4` | `glm-4-flash` |
| Qwen (DashScope) | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `qwen-plus` |
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` |

`source_lang = "auto"` auto-detects; if the text is already in `target_lang`
it flips to English (so selection-translate always does something useful).

## OCR languages

Tesseract is used for OCR. Only `eng` is installed by default. For more:

```bash
sudo apt install tesseract-ocr-chi-sim tesseract-ocr-jpn   # etc.
```

then set `ocr_langs = "eng+chi_sim"` in the config.

## Commands

```bash
ai-translate                 # popup (default)
ai-translate selection       # translate current selection
ai-translate ocr             # capture region, OCR, translate
ai-translate text "hello"    # translate to stdout (no GUI)
ai-translate daemon          # the hotkey daemon (run by systemd)
ai-translate config-path     # print config file path
```

## Service

The hotkey daemon runs as a systemd user service:

```bash
systemctl --user status app-io.github.lue.AiTranslate.service
systemctl --user restart app-io.github.lue.AiTranslate.service
```

## Build & install from source

```bash
cargo build --release
install -m755 target/release/ai-translate ~/.local/bin/ai-translate
systemctl --user restart app-io.github.lue.AiTranslate.service
```

See `DESIGN.md` for the full cross-desktop architecture (X11/Wayland, GNOME vs
KDE, Flatpak packaging) this MVP is a first slice of.
