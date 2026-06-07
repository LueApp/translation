# Ubuntu AI Translator — Master Design Document

> A desktop translation tool for Ubuntu (X11 + Wayland) that beats Google Translate by using AI models. Stack: Rust + Tauri v2, distributed as a sandboxed Flatpak on Flathub. Knowledge cutoff early 2026; facts marked _(low confidence)_ need live re-verification before coding.

---

## 1. Overview

We are building a tray-resident desktop translator for Ubuntu 22.04/24.04/25.04 (and KDE), packaged as a sandboxed Flatpak on Flathub, that exposes three triggers: a global hotkey popup (T1), translate-the-selection (T2), and capture-region-OCR-translate (T3). The translation engine is a pluggable async `trait Translator` defaulting to **LibreTranslate** (no key, self-hostable) with opt-in OpenAI / Anthropic Claude / Google Gemini / DeepL backends behind user-supplied keys, streaming partial output token-by-token to the UI. The single hardest constraint shapes the entire design: on the most common target — **Ubuntu 24.04 LTS = GNOME 46 on Wayland inside a Flatpak sandbox** — the global hotkey portal does not exist and reading another app's selection is architecturally impossible, so we cannot honestly promise "global hotkey + click-any-word" out of the box there. The design therefore routes every OS-integration call through a runtime-detected `trait Platform` that picks an X11 fast path or an XDG-portal path and degrades each capability explicitly. The one trigger that is **FULL everywhere** is T3 (capture → OCR via the compositor's own region picker), so it becomes the hero feature, with the popup/clipboard flow as the universal baseline. Because newest Ubuntu defaults to Wayland, we do **not** stop at "portal-degraded" — §2b adds a native Wayland power layer (an optional companion GNOME Shell extension + KDE `ext-data-control`) that restores FULL, prompt-free, copy-free T1+T2 on GNOME 42–48 and near-X11 ergonomics on KDE. We lead with what works, name what's blocked, and ship the workaround in-product rather than hiding the gap.

---

## 2. Hard reality: X11 vs Wayland vs Flatpak sandbox

This table is grounded in four adversarial verdicts (3 refuted/partial as marked). "Wayland" columns mean **inside the Flatpak sandbox**, which is the shipping target. Cells are **FULL / DEGRADED / BLOCKED** with the recommended fallback.

| Capability | X11 (non-sandbox or `fallback-x11`) | GNOME-Wayland (Flatpak) | KDE-Wayland (Flatpak) |
|---|---|---|---|
| **T1 Global hotkey** | **FULL** — `XGrabKey` via `tauri-plugin-global-shortcut`; app owns the combo | **GNOME ≤47 (incl. 24.04 LTS): BLOCKED.** No GlobalShortcuts portal until GNOME 48. **GNOME 48+ (25.04+): DEGRADED** — portal exists, user binds the key in Settings, one-time consent dialog, _(low confidence)_ activation-storm bugs | **DEGRADED** — GlobalShortcuts portal is mature here; user binds in Settings |
| **T2 Read other app's selection** | **FULL** — read X11 `PRIMARY` on hotkey; no copy step needed | **BLOCKED** — no portal/protocol exposes another app's selection or global input; mutter won't ship `ext-data-control` to clients | **DEGRADED** — `wl-clipboard-rs` over `ext-data-control` reads CLIPBOARD/PRIMARY directly |
| **T3 Region capture → OCR** | **FULL** — `xcap` direct framebuffer grab + own overlay | **DEGRADED (full quality)** — interactive Screenshot portal _is_ the region picker; one-time grant on GNOME 43+, then silent | **DEGRADED (full quality)** — same portal path |
| **Secret (API key) storage** | **FULL** — `oo7` → Secret Service | **FULL** — `oo7` auto-selects portal-backed encrypted file backend, per-app isolated | **FULL** — same |
| **Popup at cursor position** | **FULL** — `set_position` to global coords | **BLOCKED** — Wayland forbids client self-positioning; **center on active output** | **BLOCKED** — center |
| **Tray icon** | **FULL** (SNI/AppIndicator) | **DEGRADED** — stock GNOME has no SNI host; needs AppIndicator extension. Never make a trigger tray-dependent | **FULL** (KDE ships SNI host) |
| **Autostart** | FULL (XDG `.desktop`) | DEGRADED — `Background` portal `RequestBackground{autostart:true}`, one consent prompt | DEGRADED — same |
| **Translation backend (HTTPS/SSE)** | FULL | **FULL** — network layer is unaffected by Wayland/sandbox (needs `--share=network`) | FULL |

### What the verdicts force us to say plainly

- **T1 on Ubuntu 24.04 LTS is BLOCKED** (verdict: *refuted*, high confidence). Tauri's global-shortcut plugin is X11-only (`tao` disabled it on Wayland to avoid a libX11 segfault), and the GNOME GlobalShortcuts portal landed only in **GNOME 48**. Even on 48+, `BindShortcuts` shows a one-time consent dialog and the **user** assigns the key — so it is DEGRADED, never FULL, and "zero-setup global hotkey on GNOME-Wayland" is not achievable on the target LTS.
  - **Fallback (the universal escape hatch):** guide the user to create a GNOME *custom keyboard shortcut* in Settings that runs `flatpak run <app-id> --toggle-popup`; the `tauri-plugin-single-instance` argv callback forwards it to the running daemon. Also expose tray actions. On X11 and GNOME 48+/KDE, register the hotkey natively/via portal.

- **T2 "click any word in any app" is BLOCKED on all Wayland** (verdict: *refuted*, high confidence). No portal reads another app's selection; `wlr/ext-data-control` is not advertised to clients by mutter; synthetic Ctrl+C needs the RemoteDesktop portal (per-launch consent, dies on lock) and is impossible in-sandbox anyway; AT-SPI is blocked/uneven; the Clipboard portal only works inside a RemoteDesktop session you own.
  - **Fallback (FULL everywhere):** redesign T2 as **"select → real Ctrl+C → hotkey"**. We read our **own** clipboard, which is always allowed. On X11 we keep the magic select-then-hotkey via `PRIMARY` (no copy). On KDE-Wayland, `wl-clipboard-rs` can shorten it back to select-then-hotkey opportunistically.

- **T3 is achievable as DEGRADED-but-clean** (verdict: *partial*, med confidence). `interactive=true` makes the compositor draw the rectangle — that chrome appearing each capture is the picker, **not** a permission nag. On GNOME 43+ the *permission* persists after one grant. We do **not** use `interactive=false` (it returns full-screen and forces an in-app crop) on Wayland, and we never try a custom overlay there (can't position, can't read pixels behind it). On X11 we bypass the portal entirely for a silent grab.

- **Flatpak build + review is feasible** (verdict: *partial*, med confidence). We do **not** bundle WebKitGTK (it comes from `org.gnome.Platform`); we vendor cargo + node sources for the offline builder and ship Tesseract as a build module.

**Net product framing:** Out of the box (zero extra install) the app is a **portal-driven popup translator + screenshot-OCR**: "true global hotkey" is a GNOME-48+/KDE/X11 feature and "click any word anywhere" is an X11 power-mode. **But Wayland is the default target, so we go native — see §2b:** an optional companion GNOME Shell extension (one consent + one re-login) lifts GNOME 42–48 to FULL prompt-free hotkey + copy-free selection, and KDE gets it natively with no extension at all. Onboarding states the current level honestly per detected session.

---

## 2b. Native Wayland Strategy

On Wayland we can be **fully native for T1 and T2 only by shipping a companion GNOME Shell extension**; without it, the honest ceiling is "portal-native but degraded" on GNOME and "near-X11 native" on KDE. The strategy is layered and probed at runtime: (1) **portal-first baseline** — `org.freedesktop.portal.GlobalShortcuts` for T1 (GNOME 48+/KDE 5.27+) plus own-clipboard-read for T2 — which keeps the zero-extra-install promise; (2) an **optional GNOME-extension "power layer"** that owns a real Mutter keybinding (`Main.wm.addKeybinding`) and reads the PRIMARY selection (`St.Clipboard`/`Meta.Selection`) entirely inside the compositor, the only way to get prompt-free, copy-free T1+T2 on GNOME 42–47 where no portal exists; (3) **KDE's `ext-data-control-v1`** (via `wl-clipboard-rs`) for true copy-free selection read, which Mutter refuses but KWin 6.2+/wlroots advertise; and (4) `RemoteDesktop`+`Clipboard` portal (libei synthetic Ctrl+C) as a consent-gated last-resort accelerator. Two hopes are **refuted and routed around**: app-direct AT-SPI from inside the sandbox (the a11y bus is filtered to self-exposure only, and Chromium/Electron expose nothing by default — so AT-SPI lives only *inside* the extension, never in the app), and "prompt-free on every use" for the RemoteDesktop path (the first use *always* prompts and the Flatpak persist/remember-me path is currently broken). T3 (region OCR) remains fully native everywhere via the interactive Screenshot portal and is also the universal fallback for reading on-screen text the other paths cannot.

### Revised capability matrix (Wayland-native)

| Trigger | GNOME-Wayland, portal-only (no extension) | GNOME-Wayland + our extension | KDE Plasma Wayland (5.27 / 6.x) |
|---|---|---|---|
| **T1 global hotkey** | **GNOME 48+: DEGRADED** — `org.freedesktop.portal.GlobalShortcuts` v2; user binds/confirms the key once, `Activated` signal fires app handler. **GNOME 42–47: BLOCKED** (portal absent; only the weak custom-shortcut deep-link remains). Caveat: early GNOME 48 `Activated`/focus bugs (Chromium hit & disabled it). | **FULL** — `Main.wm.addKeybinding(name, Gio.Settings, Meta.KeyBindingFlags.NONE, Shell.ActionMode.NORMAL\|OVERVIEW, handler)`; a real Mutter grab, zero per-use prompt, fires over fullscreen, works **42–48**. Extension emits a D-Bus signal to the app. | **DEGRADED** — `GlobalShortcuts` portal via `xdg-desktop-portal-kde` (KGlobalAccel-backed, since Plasma 5.27); user picks key once, manageable in System Settings. Mature on 6.x. |
| **T2 selection / click-word** | **DEGRADED→BLOCKED** — no copy-free path: Mutter refuses `ext-data-control` (privacy policy), and app-direct AT-SPI is **BLOCKED** (sandbox proxy filters the a11y bus to self-exposure only; Chromium/Electron expose nothing). Best available: **own-clipboard read after the user presses real Ctrl+C** (universal baseline), or consent-gated `RemoteDesktop`+`Clipboard` synthetic Ctrl+C (first-use prompt mandatory, Flatpak persist currently broken). | **FULL (copy-free)** — extension reads `St.Clipboard.get_default().get_text(St.ClipboardType.PRIMARY, cb)` (= the live highlighted text in the focused app), no synthetic copy, no prompt; returned over D-Bus `GetSelection()`. Click-word degrades to PRIMARY-on-hotkey (Component hit-testing via AT-SPI is fragile/being removed by Newton). | **FULL (copy-free) on 6.2+** — `ext-data-control-v1` PRIMARY read via `wl-clipboard-rs` (`get_contents(ClipboardType::Primary, …)`); select-then-hotkey, no prompt, no copy. **DEGRADED on 5.27–6.1** — only `wlr-data-control` (often CLIPBOARD-only) or Klipper D-Bus (truncates, bug 446441). |
| **T3 region OCR** | **FULL** — interactive `org.freedesktop.portal.Screenshot` (`Screenshot(parent,{interactive:true})`), full quality, region picker each time. | **FULL** — same Screenshot portal (extension adds nothing here). | **FULL** — same Screenshot portal via `xdg-desktop-portal-kde`. |

### Recommended native stack per trigger

**T1 — global hotkey (ordered best→worst, native only):**
1. **Extension grab (DEFAULT when extension present)** — GJS `Main.wm.addKeybinding(...)` bound to a `Gio.Settings`-backed key; true compositor grab, zero prompt, fires over fullscreen, works GNOME 42–48. *This is the only FULL option and the default on GNOME whenever the extension is installed.*
2. **GlobalShortcuts portal** — `org.freedesktop.portal.GlobalShortcuts` v2 (`CreateSession`→`BindShortcuts([(id,{description,preferred_trigger})])`→`Activated(session,id,timestamp,{activation_token})`); persist `restore_token`, call `ListShortcuts` to display the *real* trigger. **Default on GNOME 48+ and KDE when the extension is absent.** Use the `activation_token` from `Activated` to legally raise the popup (focus-stealing prevention otherwise blocks it).
3. **Custom-shortcut deep-link** — documented GNOME Settings→Keyboard shortcut that execs `flatpak run … --toggle-popup` into a single-instance daemon. Floor only on GNOME 42–47 with no extension; the weak path the directive disowns, kept solely so the app is never dead.

**Default decision:** prefer the extension where installed; else GlobalShortcuts portal (GNOME 48+/KDE); else deep-link.

**T2 — selection / click-word (ordered best→worst, native only):**
1. **ext-data-control PRIMARY read** — `wl-clipboard-rs` `get_contents(ClipboardType::Primary, Seat::Unspecified, MimeType::Text)`, gated on `is_primary_selection_supported()` / registry presence of `ext_data_control_manager_v1`. Copy-free, no prompt. **Default on KDE 6.2+/wlroots.** *Impossible on GNOME — Mutter refuses the protocol.*
2. **Extension PRIMARY read** — `St.Clipboard.get_default().get_text(St.ClipboardType.PRIMARY, cb)` inside the shell, returned via D-Bus `GetSelection()→s`. Copy-free, no prompt. **Default on GNOME whenever the extension is installed** — the *only* copy-free GNOME path.
3. **RemoteDesktop + Clipboard synthetic Ctrl+C** — `RemoteDesktop.create_session`→`select_devices(KEYBOARD, persist_mode=ExplicitlyRevoked)`→`Clipboard.request(session)`→`start` (one consent)→`notify_keyboard_keysym`/`connect_to_eis`+`reis` for Ctrl+C→`Clipboard.selection_read(session,"text/plain;charset=utf-8")`. **Opt-in only:** first use *always* prompts; persist the rotating `restore_token`; gracefully fall back if the Flatpak "remember-me deadlock" hits. Save/restore the user clipboard.
4. **Own-clipboard read after real Ctrl+C** — universal baseline; user copies, app reads its own clipboard. **Default fallback on GNOME without the extension.**

**Routed-around (do NOT ship):** app-direct AT-SPI (`--talk-name=org.a11y.Bus` reviewed as self-exposure, filtered by `xdg-dbus-proxy`, blind to Chromium/Electron) — refuted; only usable *inside* the extension.

### The companion GNOME Shell extension

**Verdict: SHIP IT, as an OPTIONAL "power mode," not a hard dependency.** It is the single mechanism that turns GNOME T1+T2 from DEGRADED/BLOCKED into FULL and copy-free across GNOME 42–48 (covering the Ubuntu 24.04 LTS = GNOME 46 sweet spot where the portal does not yet exist). It does three things, all confirmed: (1) owns a GSettings-configured keybinding via `Main.wm.addKeybinding` (T1); (2) reads `St.ClipboardType.PRIMARY` for the live selection (T2, copy-free, no prompt); (3) bridges to the app over D-Bus.

**IPC direction & interface:** the **extension exports** a dedicated, minimal session-bus name (`io.github.<org>.Translator.ShellHelper`, via `Gio.DBusExportedObject.wrapJSObject` + `Gio.bus_own_name`), the **app is the client**. This needs exactly one clean finish-arg — `--talk-name=io.github.<org>.Translator.ShellHelper` — and never `--talk-name=org.gnome.Shell` (a review red flag). Interface:
```
signal HotkeyPressed(s action)        # T1
method GetSelection() -> (s text)     # T2, PRIMARY (async; resolve in St.Clipboard cb)
method GetClipboard() -> (s text)     # optional "translate clipboard"
```
Note: an app owns its own `$FLATPAK_ID` name by default, so the extension *could* call the app inbound with no extra hole — but exporting from the extension is cleaner and keeps a single, auditable name.

**Distribution / install UX (be honest — NOT zero-setup):** the extension **cannot live inside the Flatpak** and **cannot be auto-loaded into a running Wayland session**. Ship it on **EGO**; install it from the app via the `org.gnome.Shell.Extensions.InstallRemoteExtension(uuid)` D-Bus method (one GNOME confirmation dialog — the same path Extension Manager uses; add only `--talk-name=org.gnome.Shell.Extensions`, **never** `flatpak-spawn --host` or `org.freedesktop.Flatpak`). Then the user **must log out and back in once** — GNOME Shell cannot hot-load a first-time extension on Wayland (`Meta.restart`/reload are X11-only). Realistic cost = **one consent dialog + one re-login**. Until then, run the portal/own-clipboard baseline so the app is never dead.

**Maintenance cost:** the GNOME 45 ESM break is total and unavoidable — ship **two artifacts in one EGO page**: legacy `imports.*` for `shell-version` 42/43/44 and ESM for 45/46/47/48, plus a `metadata.json` bump + re-test every GNOME cycle (~6-monthly). Keep the extension ~200 LOC (keybinding + PRIMARY read + D-Bus export, no subprocesses) to survive EGO's human, adversarial review (precedent: Text Translator #593, Translate clipboard #4097, openwispr, Pano all pass). EGO may scrutinize PRIMARY-read-on-every-selection; read PRIMARY only on explicit hotkey/method call, never poll.

### Detection & architecture delta

Replace the flat `PortalBackend` on the Wayland/sandboxed arm with a composed **`WaylandNative`** backend that delegates each `Platform` method down a runtime-ranked chain of sub-providers (`HotkeyProvider`/`SelectionProvider`/`CaptureProvider`), demoting any that fail at runtime and caching the demotion.

Startup `probe()` builds a cached `CapabilitySet`:
- **session** — `XDG_SESSION_TYPE`, authoritative tiebreak `WAYLAND_DISPLAY` (never trust `DISPLAY` alone); **sandbox** — `/.flatpak-info` exists.
- **compositor + version** — `org.gnome.Shell.ShellVersion`, else `org.kde.KWin`/version, else assume wlroots.
- **portal versions** — read the `version` property on each `org.freedesktop.portal.*` interface over zbus (`GlobalShortcuts`, `RemoteDesktop`, `Clipboard`, `Screenshot`); absent ⇒ `None`.
- **extension bridge** — `NameHasOwner("io.github.<org>.Translator.ShellHelper")`; if present, call its `ApiVersion()`.
- **ext-data-control** — Wayland registry roundtrip for `ext_data_control_manager_v1`/`zwlr_data_control_manager_v1` (or `wl-clipboard-rs::is_primary_selection_supported()`).
- **libei** — `RemoteDesktop` version ≥ 2 with `ConnectToEIS`.

Decision tree per trigger (first match wins, demote on failure):
- **T1:** `ext_bridge? → ExtensionGrab` ▸ `GlobalShortcuts portal? → PortalGlobalShortcuts` ▸ `CustomShortcutDeepLink` ▸ `TrayOnly`.
- **T2:** `ext_data_control? → ExtDataControlPrimary` ▸ `ext_bridge? → ExtensionPrimary` ▸ `libei? → SyntheticCopyThenRead (opt-in)` ▸ `OwnClipboardRead`. (AT-SPi app-direct is **removed** from the chain.)
- **T3:** interactive Screenshot portal everywhere.

`MockPlatform` gains a `MockCapabilitySet` so ranking is unit-testable without a display server. The `platform_capabilities()` IPC command grows `hotkey_strategy`, `selection_strategy`, `extension_present`, `extension_action` ("install"/"enable"/"relogin"/"ok") to drive Diagnostics + onboarding copy.

### Packaging delta

`finish-args` deltas over the v1 spine:
```
--socket=wayland
--socket=fallback-x11
--talk-name=io.github.<org>.Translator.ShellHelper   # extension bridge (T1/T2 power layer)
--talk-name=org.gnome.Shell.Extensions               # InstallRemoteExtension guided install only
# org.freedesktop.portal.* is implicit — GlobalShortcuts/RemoteDesktop/Clipboard/Screenshot need NO finish-arg
# (wayland socket is all ext-data-control needs on KDE/wlroots — it is a raw protocol, not a portal)
```
**Do NOT add** `--talk-name=org.a11y.Bus` (reviewed as on-screen-text read; AT-SPI is routed through the extension, so the app keeps zero a11y holes), `--talk-name=org.gnome.Shell` (red flag), `--talk-name=org.freedesktop.Flatpak`, or `flatpak-spawn --host` (sandbox-escape, Flathub rejection). `--socket=pipewire` only if a future ScreenCast-restore T3 is added.

**Extension ships separately on EGO** (cannot live in the Flatpak); optionally bundle the zip read-only in `/app/share/<uuid>/` purely for offline reference, but install via `InstallRemoteExtension`. **Flathub verdict:** the default install must be fully self-contained and zero-extension (portal + own-clipboard) — then the extension is a discoverable enhancement and review is not jeopardized. **EGO:** two `shell-version` packages (legacy 42–44, ESM 45–48), non-minified non-obfuscated JS, D-Bus IPC (no subprocess spawning), no `eval`, PRIMARY read only on explicit action; budget for human review latency + the Dec-2025 "no AI-looking code" rule.

### Roadmap delta

- **M0 (spikes):** ADD a **GNOME Shell extension spike** — prove `Main.wm.addKeybinding` + `St.Clipboard` PRIMARY + `Gio.DBusExportedObject` bridge on GNOME 46 (LTS) and 48. ADD a **GlobalShortcuts portal spike on a real GNOME 48 box** — verify `Activated` fires, `activation_token` raises the popup, and `restore_token` suppresses re-prompt (early-48 bugs). ADD an **`ext-data-control`/`wl-clipboard-rs` PRIMARY spike on KDE 6.2+**. ADD a **RemoteDesktop+Clipboard persist spike** to confirm/deny the Flatpak "remember-me deadlock."
- **M1 (platform layer):** Replace flat `PortalBackend` with composed **`WaylandNative`** + `CapabilitySet` probe and per-trigger ranking; add `MockCapabilitySet`. **Pull native T1/T2 earlier** — they are no longer "fallback-only," they are the primary Wayland path.
- **M2 (triggers):** T1 GlobalShortcuts portal + T2 own-clipboard baseline land here (zero-extension, ships first). T3 Screenshot portal unchanged.
- **M3 (extension power layer):** Build/publish the two-artifact EGO extension; implement the app-side zbus client + `InstallRemoteExtension` guided onboarding (consent + re-login wording). Add KDE `ext-data-control` copy-free T2.
- **M4:** Opt-in `RemoteDesktop`+libei synthetic-copy T2 (behind explicit consent, below own-clipboard), persist `restore_token` in oo7. Diagnostics tab surfacing detected strategies.
- **M5:** Per-GNOME-version CI matrix (42/44/46/48 VMs) for the extension; recurring per-cycle re-test + EGO re-upload budgeted as ongoing maintenance.
- **DROP** from scope: app-direct AT-SPI (refuted); keep only as extension-internal read.

### Honest verdict

**(a) Stock Ubuntu 24.04 / GNOME 46:** out of the box the app gives a degraded T1 (no portal — user hand-binds a custom shortcut deep-link) and a copy-then-translate T2; **install our extension + one re-login** and you get FULL native T1 (real Mutter hotkey, fires over fullscreen) and FULL copy-free T2 (PRIMARY read) — genuinely native, at a two-action setup cost. **(b) Ubuntu 25.04 / GNOME 48:** zero-extension T1 is native-but-degraded (one-time bind via GlobalShortcuts portal, with early-48 reliability caveats), T2 stays copy-then-translate because Mutter still refuses `ext-data-control`; the extension still wins for prompt-free T1 and copy-free T2. **(c) KDE Plasma (esp. 6.2+):** the best Wayland experience with no extension at all — mature GlobalShortcuts portal for T1 and copy-free `ext-data-control` PRIMARY read for T2, approaching X11 ergonomics. **What remains impossible natively:** copy-free selection read on GNOME *without* the extension (Mutter's permanent privacy stance), prompt-free synthetic-copy on first use anywhere, reliable cross-app "click any word" hit-testing on Wayland (AT-SPI absolute coords are being removed by Newton), and sandboxed app-direct AT-SPI — for all of these, the extension (GNOME) or T3 Screenshot→OCR (universal) is the honest answer.

Sources: [XDG GlobalShortcuts portal docs](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html), [Chromium GlobalShortcuts misbehaving on GNOME 48](https://issues.chromium.org/issues/404298968), [Extension Manager (Flatpak driving org.gnome.Shell.Extensions)](https://github.com/mjakeman/extension-manager), [wl-clipboard ext-data-control / mutter refusal discussion](https://github.com/bugaevc/wl-clipboard/issues/242).

---

## 3. Architecture

### 3.1 The platform-abstraction spine

One object-safe, async `trait Platform` (via `async-trait`) behind `Arc<dyn Platform>`, chosen once at startup. It isolates the three capabilities that behave differently per display server. Every probe returns a 3-state `Capability` — never a bool — because "the call exists but the compositor refuses / re-prompts / returns nothing" is the common case.

```rust
pub enum Capability { Full, Degraded(DegradeReason), Blocked(BlockReason) }
pub enum DegradeReason { OneTimeConsent, UserMustBindInUI, RequiresSyntheticCopy, PrimarySelectionOnly }
pub enum BlockReason  { PortalMissing(&'static str), CompositorRefuses, SandboxForbids, NoPipewire }

#[async_trait::async_trait]
pub trait Platform: Send + Sync {
    fn session(&self) -> SessionKind;            // X11 | Wayland | Unknown
    fn is_sandboxed(&self) -> bool;              // /.flatpak-info present?

    // Cheap probes — drive onboarding UI and the Diagnostics tab.
    async fn probe_hotkey(&self) -> Capability;
    async fn probe_selection(&self) -> Capability;
    async fn probe_capture(&self) -> Capability;

    // T1: returns a stream of activations + the *effective* trigger (None on X11; Some(desc) via portal).
    async fn register_hotkey(&self, id: &str, accel: Accelerator, desc: &str)
        -> Result<HotkeyRegistration, PlatformError>;

    // T2: read the text the user is pointing at (PRIMARY on X11; own-clipboard on Wayland).
    async fn read_selection(&self, mode: SelectionMode) -> Result<String, PlatformError>;

    // T3: cropped RGBA frame for OCR.
    async fn capture_region(&self, hint: RegionHint) -> Result<Frame, PlatformError>;

    fn can_anchor_popup_at_cursor(&self) -> bool; // X11: true, Wayland: false
}
```

**Detection** (order matters — the sandbox and XWayland are the traps):

```rust
pub async fn detect_platform() -> Arc<dyn Platform> {
    let sandboxed = Path::new("/.flatpak-info").exists() || env::var_os("FLATPAK_ID").is_some();
    // In a Flatpak sandbox, X11 grabs/dev access are gone: portals are the ONLY legal path,
    // even on an X11 session. Sandbox forces the portal backend.
    match SessionKind::detect() {
        SessionKind::Wayland => Arc::new(PortalBackend::new().await),
        SessionKind::X11 if sandboxed => Arc::new(PortalBackend::new().await),
        SessionKind::X11 => Arc::new(X11Backend::new()),     // non-sandbox dev/AppImage
        SessionKind::Unknown => Arc::new(PortalBackend::new().await),
    }
}
// SessionKind::detect(): prefer XDG_SESSION_TYPE; use WAYLAND_DISPLAY as the authoritative
// tiebreak (XWayland sets DISPLAY=:0 even on Wayland — never conclude X11 from DISPLAY alone).
```

Branch core logic on **live portal-interface availability** (read the `version` property over zbus), not on `XDG_CURRENT_DESKTOP`. Detect GNOME version via `org.gnome.Shell` `ShellVersion` only to *explain* degradation and gate the T1 portal path. A `MockPlatform` makes the trait unit-testable without a display server.

### 3.2 Cargo workspace / module map

Single Tauri binary, single instance, tray-resident daemon. Webview windows are created hidden and shown on demand for instant popups.

```
ai-translate/
├─ Cargo.toml                      # workspace
├─ flatpak/
│  ├─ org.example.AiTranslate.yaml # manifest
│  ├─ cargo-sources.json           # flatpak-cargo-generator output (committed)
│  └─ node-sources.json            # flatpak-node-generator output (committed)
├─ data/                           # .desktop, metainfo.xml, icons
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs / lib.rs          # tauri::Builder, plugin registration, setup()
│  │  ├─ state.rs                  # AppState: Settings, provider registry, in-flight cancel tokens
│  │  ├─ commands/                 # #[tauri::command] surface (§6.3)
│  │  │  ├─ translate.rs           # translate_stream(), cancel_translation()
│  │  │  ├─ triggers.rs            # capture_region_ocr(), read_clipboard_text(), show_popup_with()
│  │  │  ├─ settings.rs            # get/set settings, list/test providers
│  │  │  └─ window.rs              # open_settings, hide_popup, pin_popup, copy_result
│  │  ├─ platform/                 # ★ the capability spine
│  │  │  ├─ mod.rs                 # trait Platform, Capability, detect_platform()
│  │  │  ├─ detect.rs              # SessionKind::detect, portal_has(), gnome_version()
│  │  │  ├─ x11/                   # X11Backend (cfg unix, feature "x11"): XGrabKey, xcap, PRIMARY
│  │  │  └─ portal/
│  │  │     ├─ mod.rs              # PortalBackend, shared zbus::Connection
│  │  │     ├─ hotkey.rs          # GlobalShortcuts via ashpd
│  │  │     ├─ capture.rs         # interactive Screenshot via ashpd
│  │  │     └─ selection.rs       # own-clipboard read; KDE ext-data-control accelerator
│  │  ├─ ocr/                      # trait OcrEngine
│  │  │  ├─ tesseract.rs          # leptess (bundled tessdata_fast) — offline default
│  │  │  └─ vision_llm.rs         # opt-in: reuse provider key for vision OCR
│  │  ├─ providers/                # translation backends (§5)
│  │  │  ├─ mod.rs                 # trait Translator, FallbackTranslator, ProviderCaps
│  │  │  ├─ libretranslate.rs deepl.rs anthropic.rs openai.rs gemini.rs
│  │  ├─ secrets.rs                # oo7 keyring wrapper
│  │  ├─ config.rs                 # serde + toml config schema, migrations
│  │  └─ ipc.rs                    # event names, payload structs
│  └─ tauri.conf.json
└─ ui/                             # Svelte 5 + Vite + TS (popup.html, settings.html)
```

---

## 4. Per-trigger design

Every trigger funnels to the same point: it produces `source_text: String` (+ optional `src_lang`) handed to the translation backend (§5). The hotkey itself is the entry point for all three on the keyboard path.

### 4.1 T1 — Global hotkey → popup

| Display server | Mechanism | Crate / interface |
|---|---|---|
| X11 (non-sandbox) | `XGrabKey` | `tauri-plugin-global-shortcut` (→ `global-hotkey`). App owns the combo; `effective_trigger = None`. Handle `BadAccess` (combo taken) and default to an uncommon combo (`Super+Shift+T`). |
| GNOME 48+ / KDE Wayland | `org.freedesktop.portal.GlobalShortcuts` | `ashpd::desktop::global_shortcuts`: `CreateSession` → `BindShortcuts([NewShortcut::new("toggle-popup","Open translation popup").preferred_trigger("CTRL+ALT+t")])` → consume `receive_activated()`. `preferred_trigger` is a **hint**; call `ListShortcuts` and show the *actual* bound trigger. Persist `restore_token`. Use the `Activated` signal's **`activation_token`** to let the popup steal focus. |
| GNOME ≤47 (24.04 LTS) | **none** | **BLOCKED.** Fall back to: (1) tray menu actions, and (2) a documented GNOME custom shortcut → `flatpak run <id> --toggle-popup`, forwarded by `tauri-plugin-single-instance`. Show a one-time banner: "System-wide hotkeys need Ubuntu 25.04+/KDE; set up a custom shortcut, or use the tray." |

**Never call the Tauri global-shortcut plugin on Wayland** — it silently no-ops (or segfaults). Branch in `HotkeyBackend::detect()` on session + portal availability, never blindly.

### 4.2 T2 — Translate the selection

Layered `SelectionProvider`, runtime-detected, falling through to the universal safety net:

```rust
fn capture_selection(plat: &dyn Platform) -> Option<String> {
    // Layer 1 — automatic, no copy:
    if plat.session().is_x11() && !plat.is_sandboxed() { if let Ok(t)=read_primary_x11() { return Some(t); } } // FULL
    if has_ext_data_control()  /* KDE/wlroots */         { if let Ok(t)=read_primary_wl()  { return Some(t); } } // wl-clipboard-rs
    if cfg.atspi_experimental                            { if let Ok(t)=read_atspi_caret() { return Some(t); } } // opt-in, OFF by default
    // Layer 2 — universal: user pressed real Ctrl+C; read OUR OWN clipboard (always allowed):
    read_own_clipboard().ok()  // tauri-plugin-clipboard-manager
}
```

- **X11:** read `PRIMARY` on hotkey press → instant translate, **no copy**. Crates: `x11rb` / `x11-clipboard` / `arboard` (`LinuxClipboardKind::Primary`). Optionally XFIXES `SelectionOwnerNotify` for a floating-icon UX (X11-only).
- **GNOME-Wayland (Flatpak):** **BLOCKED** for automatic capture. Ship the honest flow: *"Select text, press Ctrl+C, then press your shortcut."* On hotkey we read our own clipboard via the toolkit/clipboard-manager (no portal snooping, no synthetic input).
- **KDE/wlroots-Wayland:** `wl-clipboard-rs` over `ext-data-control` can read CLIPBOARD/PRIMARY directly → degrade back toward select-then-hotkey when `is_primary_selection_supported()`.
- **AT-SPI** (`atspi` crate, `Text.GetTextAtOffset` at caret): the only thing that can read Wayland-native apps without a copy, but coverage is uneven (Qt needs `QT_ACCESSIBILITY=1`, Electron needs a11y flags, terminals expose nothing). Ship as **opt-in "experimental auto-read", OFF by default**, always falling back to clipboard. Flatpak reaches the a11y bus via the runtime's `AT_SPI_BUS_ADDRESS` proxy.

Do **not** use the Clipboard portal for normal reads (it only relays clipboard inside a RemoteDesktop/ScreenCast session you own). Do **not** synthesize Ctrl+C on Wayland-in-sandbox.

### 4.3 T3 — Capture region → OCR → translate (the hero feature, FULL-quality everywhere)

| Display server | Mechanism | Crate / interface |
|---|---|---|
| Wayland (GNOME/KDE, Flatpak) | **interactive Screenshot portal** | `ashpd::desktop::screenshot::Screenshot::request().interactive(true).modal(true).send()` → response `uri()` is a **file:// to an already-cropped PNG**. The compositor draws the rectangle picker. Parse the URI with `url`, decode with `image`. |
| X11 (non-sandbox) | direct grab + own overlay | `xcap` framebuffer grab; transparent fullscreen Tauri overlay for the rubber-band; crop in Rust (multiply rect by `scaleFactor`). Portal remains the runtime fallback. |

Do **not** use `interactive=false` on Wayland (returns full-screen, forces in-app crop) and never a custom overlay there. **Gotcha:** GNOME won't show the dialog if the app has no focused window — present/focus a window before invoking the portal from the hotkey path. Reserve the heavier ScreenCast+PipeWire path for a future live-translate feature only.

**OCR pipeline** (`OcrEngine` trait, runs on `tokio::task::spawn_blocking`):
1. **Preprocess** (the biggest quality lever): 2–3× upscale, grayscale, adaptive/Otsu threshold, optional invert for light-on-dark UI. Crates: `image` + `imageproc`.
2. **Default = offline Tesseract**: `leptess` (`set_image_from_mem` → `get_utf8_text`), bundled **`tessdata_fast`** (not `_best`), `TESSDATA_PREFIX=/app/share/tessdata`. Multi-lang via `eng+deu+jpn`; run `osd` for script autodetect, but let the user pin the source language. Lazy-download extra langs to `XDG_DATA_HOME`. _(med confidence on `leptess` building against runtime Tesseract 5 — keep `tesseract-rs` as a Cargo-feature fallback.)_
3. **Opt-in vision-LLM OCR**: reuse the provider key; one combined "extract and translate to {target}" call cuts a round trip. Surface a per-result "Re-OCR with AI (more accurate)" button. Gate `--share=network` framing accordingly.

---

## 5. Translation backend

### 5.1 The trait

Tauri v2 IPC **cannot stream a return value**, so all streaming happens in Rust and partials are pushed to the webview via a request-scoped `tauri::ipc::Channel<T>` (§6.3). The trait streams chunks; non-streaming providers emit exactly one `done=true` chunk so the UI code is identical for all.

```rust
#[async_trait]
pub trait Translator: Send + Sync {
    fn id(&self) -> ProviderId;
    fn caps(&self) -> ProviderCaps;     // { streaming, glossary, formality, detect, max_chars }
    async fn translate(&self, req: TranslateRequest)
        -> Result<BoxStream<'static, Result<TranslationChunk, TranslateError>>, TranslateError>;
    async fn detect(&self, text: &str) -> Result<LangCode, TranslateError> { Err(/* unsupported */) }
    async fn languages(&self) -> Result<Vec<LangCode>, TranslateError>;
}

pub struct TranslationChunk { pub delta: String, pub detected_src: Option<LangCode>, pub done: bool }
pub enum TranslateError { Auth(String), RateLimited(Option<Duration>), Timeout,
    Network(String), Provider{code:u16,msg:String}, UnsupportedPair(String,String), Config(String) }
```

Errors are a **normalized enum** so the fallback orchestrator can decide: retry transient classes (`RateLimited`, `Network`, `Provider{5xx}`, `Timeout`) with jittered backoff honoring `Retry-After`; **never** retry `Auth` / `UnsupportedPair` / other 4xx.

### 5.2 Providers (one pooled `reqwest::Client`; `--share=network`)

- **LibreTranslate (default, no key).** `POST {base}/translate` `{q, source:"auto"|code, target, format}`; response `translatedText` + `detectedLanguage`. No streaming → one chunk. Default `base_url` configurable to `http://localhost:5000`. The always-available tail of the fallback chain.
- **Anthropic Claude (key, SSE).** `POST https://api.anthropic.com/v1/messages`, headers `x-api-key` + `anthropic-version: 2023-06-01`, `"stream": true`. **Default model `claude-opus-4-8`** ($5/$25 per 1M); expose `claude-haiku-4-5` ($1/$5) and `claude-sonnet-4-6` for latency/cost. Parse `content_block_delta` → `delta.text` → chunk; `message_stop` → `done`. **Do NOT send `temperature`/`top_p`/`top_k` or `budget_tokens`** — Opus 4.8 returns 400. Leave `thinking` omitted (off) for translation. Call the Messages API directly over `reqwest` SSE rather than pulling in an SDK, to keep one streaming abstraction across all five providers.
- **OpenAI (key, SSE).** `chat/completions`, `Authorization: Bearer`, `delta.content`, terminated by `data: [DONE]`. Model id config-driven (catalog drifts); `temperature: 0`.
- **Google Gemini (key, SSE).** `:streamGenerateContent?alt=sse`, header `x-goog-api-key`, extract `candidates[0].content.parts[0].text`. Default a current `gemini-2.5-flash`, config-driven.
- **DeepL (key, non-streaming).** `/v2/translate`, `Authorization: DeepL-Auth-Key`. **Route by key suffix:** `:fx` ⇒ `api-free.deepl.com`, else `api.deepl.com` (encode in the impl, never user config). `formality` → `more`/`less`/`default` (gate on supported target langs). One `done` chunk.

### 5.3 LLM prompt design (shared across Claude/OpenAI/Gemini)

System prompt: *"You are a professional translator. Translate into {target}. Output ONLY the translation — no preamble, notes, or quotes. Preserve formatting, line breaks, markdown, code blocks verbatim. Preserve tone ({tone_directive}). Keep URLs/emails/@mentions/#hashtags/numbers/proper nouns unchanged. If already in {target}, return unchanged. {glossary_block}"*. **Delimit untrusted captured text** (T2/T3 feed arbitrary content that may contain "ignore previous instructions") inside `<<<TEXT>>> … <<<TEXT>>>` markers and instruct the model to translate only the delimited block.

### 5.4 Fallback + detection

`FallbackTranslator { chain: Vec<Arc<dyn Translator>> }` (e.g. `[DeepL, Claude, LibreTranslate]`). Advance to the next provider on pre-first-chunk failure only; **once a delta has streamed you cannot transparently fall back** — surface the error and offer retry. LibreTranslate-local is the always-available tail. Detection is tiered: provider-native auto-detect → local `whatlang` (offline, for UI prefill) → LibreTranslate `/detect`; always advisory, never blocking.

### 5.5 Secure key storage — `oo7` (recommend over plaintext and over the `keyring` default)

`oo7::Keyring::new()` **auto-detects the sandbox** and uses the **portal-backed encrypted file backend** (`org.freedesktop.portal.Secret`), giving per-app-isolated, encrypted-at-rest secrets with **no extra finish-arg** and **no** `--talk-name=org.freedesktop.secrets` hole (which would expose keys to other sandboxed apps — a Flathub red flag). Keys live under `~/.var/app/<id>/data/keyrings/`. Plaintext fallback only on explicit user acknowledgement, `chmod 600`, marked `storage="plaintext"` with a persistent UI warning — never silently.

### 5.6 Config schema (non-secret) — `~/.var/app/<id>/config/ai-translate/config.toml`

```toml
schema_version = 1
default_target_lang = "en"
default_source_lang = "auto"
default_tone = "default"                 # default | formal | informal
provider_order = ["deepl", "anthropic", "libretranslate"]   # fallback chain; keys live in the keyring
active_provider = "libretranslate"

[providers.libretranslate]  base_url = "https://libretranslate.com"   # or http://localhost:5000
[providers.anthropic]       model = "claude-opus-4-8"  max_tokens = 4096  key_ref = "secret-service"
[providers.openai]          model = "gpt-4o-mini"      key_ref = "secret-service"
[providers.gemini]          model = "gemini-2.5-flash" key_ref = "secret-service"
[providers.deepl]           formality_default = "default"  key_ref = "secret-service"   # host auto-selected by :fx suffix

[network]  request_timeout_secs = 30  stream_idle_timeout_secs = 15  max_retries = 2

[hotkeys]   # IDs + suggested triggers; actual binding owned by the compositor/portal on Wayland
popup_translate     = { id = "popup",     suggested = "CTRL+ALT+T" }   # T1
translate_selection = { id = "selection", suggested = "CTRL+ALT+S" }   # T2
ocr_region          = { id = "ocr",       suggested = "CTRL+ALT+R" }   # T3

[ocr]  engine = "tesseract"  langs = ["eng"]  atspi_experimental = false
```

`key_ref` is a marker only — **no secrets in this file**. `schema_version` drives a migration-on-load; unknown keys ignored with a warning.

---

## 6. App & UX

### 6.1 Tray, popup, settings

- **Tray** (`TrayIconBuilder` → `tray-icon` → libayatana-appindicator): Compose · Translate clipboard · Capture & OCR · Provider radio submenu · Settings · Quit. Left-click opens the Compose popup. **Probe `org.kde.StatusNotifierWatcher`**; if absent (stock GNOME), show a one-time notice to install the AppIndicator extension. **No trigger may depend on the tray** — all tray actions are also reachable via hotkey / `.desktop` actions / CLI flags.
- **Popup** — frameless, `always_on_top`, `skip_taskbar`, `transparent` (rounded corners), `resizable:false`. **Create once hidden at startup; show/hide (not close)** for instant reappearance.
  - **Positioning:** X11 → anchor near cursor (`set_position` + clamp to work area). **Wayland → `center()` on the active output** (client self-positioning is forbidden; do not advertise at-cursor cross-session).
  - **Focus:** pass the portal `Activated` `activation_token` when showing, so the Wayland compositor allows focus.
  - **Dismiss-on-blur** with a 120 ms debounce + a "pin" toggle (a `<select>` opening can momentarily blur on some compositors). Hide on `Escape`.
  - **Contents:** source textarea (pre-filled from clipboard/OCR/selection), live streaming target region, source/target lang pickers (with "auto"), swap, provider switcher, copy, pin, gear→settings. Per-session empty/error states: the Wayland T2 hint, the "GNOME 48+ required for in-app hotkey" banner.
  - _(low confidence)_ Known GNOME bug: a frameless window created `visible(false)` then shown can go input-unresponsive — if hit, create visible-offscreen then hide. Test on GNOME specifically.
- **Settings window** — standard decorated webview, lazy. Tabs: General (langs, theme, autostart) · Hotkeys (X11 in-app capture; Wayland deep-link to portal/Settings) · Providers (key entry + Test) · OCR (langs, engine) · **Diagnostics** (renders the live capability matrix for the running session — invaluable for support).
- **Single instance / autostart:** `tauri-plugin-single-instance` (argv → `--toggle-popup`/`--translate-clipboard`/`--ocr`, doubling as the GNOME-shortcut fallback). Autostart: non-Flatpak `tauri-plugin-autostart`; **Flatpak → `Background` portal `RequestBackground{autostart:true}`** (one consent prompt; never write `~/.config/autostart` directly).

### 6.2 UI framework

**Svelte 5 + Vite + TypeScript.** The popup must paint <50 ms on show — Svelte compiles to minimal JS (no vDOM runtime), reactivity maps cleanly to the streaming chunk model (`$state` string the channel handler appends to). Same framework for the (form-heavier) settings window. No SPA router; hand-rolled CSS-variable theming.

### 6.3 IPC surface

**Commands** (webview → Rust): `translate_stream(req, on_chunk: Channel<TranslationChunk>) -> StreamId` · `cancel_translation(id)` · `get_settings` / `set_settings` · `list_providers` / `test_provider` · `read_clipboard_text` · `capture_region_ocr` · `swap_languages` · `copy_result` · `open_settings` / `hide_popup` / `pin_popup` · `platform_capabilities() -> Caps{session,in_flatpak,hotkey_mode,can_anchor_cursor,can_read_selection,gnome_version,tray_host_present}` (lets the UI render honest per-platform hints).

**Streaming pattern** (the v2 boundary): the command spawns a task that reads the provider SSE stream and pushes each delta through the request-scoped `Channel` (preferred over `app.emit` — typed, no global-name collisions when two translations run). Cancellation via a `tokio_util::CancellationToken` per `StreamId`; closing the popup aborts the in-flight stream and drops the reqwest connection.

**Events** (Rust → webview, `app.emit`): `popup://open{kind,prefill}` · `ocr://progress{stage}` · `settings://changed` · `hotkey://unavailable{reason}` (UI shows the GNOME-<48 fallback) · `provider://switched`.

---

## 7. Flatpak packaging

### 7.1 Runtime + SDK

```yaml
app-id: org.example.AiTranslate
runtime: org.gnome.Platform            # ships WebKitGTK 4.1 that Tauri v2 needs
runtime-version: "48"                  # 48/24.08 base unlocks the GlobalShortcuts portal backend
sdk: org.gnome.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable   # branch MUST match the fdo base, NOT the GNOME number
  - org.freedesktop.Sdk.Extension.node20        #   GNOME 46→23.08, GNOME 47/48→24.08
command: ai-translate
```

**Load-bearing version rule:** the rust/node extension branches track the **freedesktop base** (GNOME 46 ⇒ `23.08`; 47/48 ⇒ `24.08`), *not* the GNOME number — `rust-stable//46` will not resolve. Targeting runtime `//48` is independent of the host (Flatpak runtimes are host-independent), so we get the portal backend even on a 24.04 host; re-verify the base with `flatpak info` at build time. Do **not** bundle WebKitGTK.

### 7.2 Offline vendoring (Flathub builders have no network)

- **Rust:** `flatpak-cargo-generator.py ./src-tauri/Cargo.lock -o cargo-sources.json` (committed); build with `CARGO_NET_OFFLINE=true cargo build --release --offline`.
- **Frontend:** `flatpak-node-generator pnpm pnpm-lock.yaml -o node-sources.json`; pin **pnpm** (avoid the npm `--offline` breakage); `pnpm install --offline --frozen-lockfile && pnpm build`.
- Ship a valid AppStream `*.metainfo.xml` (id, SPDX license, screenshots, OARS) and `.desktop` + hicolor icon — review rejects without them. Run `flatpak-builder-lint` locally; warnings are fatal.

### 7.3 The exact finish-args list

```
--socket=wayland               # primary path
--socket=fallback-x11          # X11 only when not on Wayland (NOT raw --socket=x11)
--share=ipc                    # X11 shared memory
--device=dri                   # GPU for WebKitGTK
--share=network                # online translation backends (justify in Flathub metadata)
--talk-name=org.kde.StatusNotifierWatcher   # tray
--talk-name=org.freedesktop.Notifications    # notifications
# Portals (GlobalShortcuts, Screenshot, Background, Secret) need NO finish-arg —
#   org.freedesktop.portal.* has implicit talk access. This is the key packaging fact.
```

**Deliberately NOT present** (each is a Flathub red flag and unnecessary): `--filesystem=host`, raw `--socket=x11`, `--talk-name=org.freedesktop.secrets` (use the Secret *portal* via `oo7` instead), `--device=all`/`/dev/input`, `--talk-name=org.gnome.Shell`. Set `WEBKIT_DISABLE_DMABUF_RENDERER=1` in the launcher `Exec` to avoid the known blank-screen on GNOME/NVIDIA/Wayland.

### 7.4 Bundling Tesseract

Build `leptonica` then `tesseract` (5.x) as autotools modules with pinned `sha256` source tarballs; install curated `tessdata_fast` `.traineddata` into `/app/share/tessdata` and set `TESSDATA_PREFIX=/app/share` (the **parent** of `tessdata`, a common footgun). Ship a small core set (eng + a few common langs); lazy-download the rest to `XDG_DATA_HOME`. `leptess` links the system libs at build time; if its build breaks against runtime Tesseract 5, switch to `tesseract-rs` behind a Cargo feature.

---

## 8. MVP roadmap

Build a thin end-to-end vertical slice first (one trigger → translate → result), then de-risk the platform-specific hard parts, then layer. **The three highest-risk spikes are M0 — prototype them before committing to the full build**, because each can invalidate the UX promise.

| Milestone | Scope | Why this order | Rough effort |
|---|---|---|---|
| **M0 — Risk spikes (do FIRST, in parallel)** | (a) `ashpd` GlobalShortcuts on a **real GNOME 48 / KDE Wayland** box — confirm activation reliability + restore_token + activation-token focus; (b) `ashpd` interactive Screenshot on **GNOME 46 *and* 48** — confirm cropped-PNG URI + one-grant persistence; (c) full **offline Flatpak CI build** (cargo+node vendoring + Tesseract module + webkit-from-runtime) producing a runnable bundle. | These three are the load-bearing unknowns: T1's real-world reliability, T3's exact portal shape, and whether the whole thing even builds on Flathub. If any fails, the design pivots. Cheaper to learn now than after building the app around them. | 1–1.5 wk |
| **M1 — Thin slice** | Tray daemon + hidden popup; type/paste text → **LibreTranslate** → result rendered (non-streaming). `Platform`/`detect_platform()` skeleton with X11 + portal backends and `MockPlatform`. Config load/save. | Proves the whole pipeline (UI ⇄ IPC ⇄ backend ⇄ render) with zero platform magic and zero keys. Smallest shippable loop. | 1 wk |
| **M2 — T3 hero feature** | Interactive Screenshot portal → preprocess → bundled Tesseract → translate → popup. X11 overlay path behind a feature flag. `ocr://progress`. | T3 is **FULL-quality everywhere** — the strongest, most demoable capability, and it validates the M0 spikes end-to-end. | 1.5 wk |
| **M3 — Streaming + LLM providers + keys** | `translate_stream` via `Channel`; Claude/OpenAI/Gemini/DeepL impls; `oo7` key storage; `FallbackTranslator`; `whatlang` detect; provider switcher + Test in settings. | Delivers the "beats Google Translate" quality story; streaming is the UX differentiator. Keys/secrets land with the providers that need them. | 2 wk |
| **M4 — T1 + T2 with honest degradation** | Hotkey: X11 plugin / portal (48+/KDE) / GNOME-46 custom-shortcut fallback via single-instance. T2: X11 PRIMARY / KDE ext-data-control / universal copy-then-hotkey; AT-SPI opt-in. `platform_capabilities` drives onboarding banners + Diagnostics tab. | These are the most environment-dependent and partly-blocked triggers; they layer on the working core and need the M0 reliability data to set expectations correctly. | 2 wk |
| **M5 — Polish + Flathub submission** | Autostart (Background portal), metainfo/screenshots, `flatpak-builder-lint` clean, glossary/formality, vision-LLM OCR opt-in, onboarding copy per session. Submit to Flathub. | Ship. Everything reviewer-facing and the nice-to-haves that don't gate the core experience. | 1.5 wk |

---

## 9. Top risks & open decisions

**Risks (each with a recommended resolution):**

- **T1 BLOCKED on Ubuntu 24.04 LTS (the biggest install base).** _Resolution:_ detect at startup; lead with T3 + tray + the documented GNOME-custom-shortcut → `--toggle-popup` fallback; treat true global hotkey as a GNOME-48+/KDE/X11 feature and say so in onboarding. Do not market "global hotkey works everywhere."
- **T2 "click any word" impossible on Wayland-in-sandbox.** _Resolution:_ ship "select → Ctrl+C → hotkey" as the Wayland baseline (FULL everywhere), keep true PRIMARY select-to-translate as an X11/KDE nicety, set expectations in onboarding. Don't promise the magic floating icon cross-session.
- **OCR accuracy on low-DPI/CJK/light-on-dark undercuts the "beat Google" promise.** _Resolution:_ mandatory preprocessing (2–3× upscale, threshold, invert), user-pinnable source language over OSD autodetect, and the opt-in vision-LLM OCR for hard cases.
- **`leptess` may not build against the runtime's Tesseract 5.** _Resolution:_ pin Tesseract/Leptonica as build modules, keep `tesseract-rs` as a Cargo-feature fallback, gate everything in the M0 CI Flatpak build.
- **Privacy/Flathub scrutiny of `--share=network` + sending text to third parties.** _Resolution:_ default to self-hostable LibreTranslate, show the active provider/endpoint in the popup, send only on explicit user action, document the data flow in Diagnostics, justify network in metainfo.
- **`ashpd`/`zbus` are pre-1.0; tokio feature must match Tauri.** _Resolution:_ pin `ashpd=0.13.x` + `zbus ^4`, enable only the `tokio` feature, isolate all portal calls behind `PortalBackend` so an upgrade touches one module.
- **XWayland detection ambiguity (DISPLAY=:0 on Wayland).** _Resolution:_ `/.flatpak-info` forces portal backend; else `XDG_SESSION_TYPE`; `WAYLAND_DISPLAY` is the authoritative tiebreak — never conclude X11 from `DISPLAY` alone.

**Open decisions (recommended call in brackets):**

- **Minimum T1 baseline on 24.04?** _[Officially support 24.04 with DEGRADED T1 (tray + custom-shortcut), require GNOME 48+/KDE/X11 for in-app hotkey. Don't gate the whole app on GNOME 48.]_
- **Ship a non-Flatpak X11 power build** that unlocks FULL T2 + cursor-anchored popups? _[Yes, later — an AppImage/.deb for power users, after the Flatpak ships; accept the dual-distribution cost only if demand appears.]_
- **Default LibreTranslate endpoint** — host one (cost/abuse) or require self-host? _[Ship a configurable default pointing at the public instance, prominently document self-hosting / `localhost:5000`; do not operate our own server initially.]_
- **OCR+translate as one vision-LLM call vs two stages?** _[Two stages by default so the user sees the OCR'd source; offer a combined call as a latency optimization in the AI-OCR path.]_
- **OpenAI/Gemini default model IDs** (catalog drifts). _[Config-driven defaults + in-app model picker; only Anthropic IDs are pinned from the verified table — `claude-opus-4-8` default.]_
- **Runtime version: target `//48` now?** _[Yes — host-independent, unlocks the GlobalShortcuts portal backend; keep the 23.08/24.08 base-mapping documented for bumps.]_
