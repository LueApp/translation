use crate::config::Config;
use crate::{capture, translate};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

type TranslationMessage = Result<translate::Translation, String>;
type ClipboardMessage = Result<(), String>;

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

pub struct TranslatorApp {
    cfg: Config,
    input: String,
    result: String,
    status: String,
    auto_copy: bool,
    show_settings: bool,
    pending: Option<Receiver<TranslationMessage>>,
    clipboard_pending: Option<Receiver<ClipboardMessage>>,
    clipboard_success_status: String,
}

impl TranslatorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        initial: String,
        auto: bool,
        auto_copy: bool,
    ) -> Self {
        // egui's built-in fonts do not cover the whole IPA block. Add a broad
        // Latin/Unicode fallback first so pronunciations such as /ˈɪmɪdʒɪŋ/
        // render instead of containing replacement boxes.
        let mut fonts = egui::FontDefinitions::default();
        let unicode_candidates = [
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        for path in unicode_candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "unicode".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(fam)
                        .or_default()
                        .insert(0, "unicode".to_owned());
                }
                break;
            }
        }

        // Load a CJK-capable font after the Unicode fallback so
        // Chinese/Japanese/Korean continue to render as well.
        let cjk_candidates = [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ];
        for path in cjk_candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "cjk".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts.families.entry(fam).or_default().push("cjk".to_owned());
                }
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);

        // Scale fonts to the configured size (base 16).
        let mut style = (*cc.egui_ctx.global_style()).clone();
        let scale = (cfg.font_size / 16.0).max(0.5);
        for (_ts, font_id) in style.text_styles.iter_mut() {
            font_id.size = (font_id.size * scale).max(9.0);
        }
        cc.egui_ctx.set_global_style(style);

        let mut app = TranslatorApp {
            cfg,
            input: initial,
            result: String::new(),
            status: String::new(),
            auto_copy,
            show_settings: false,
            pending: None,
            clipboard_pending: None,
            clipboard_success_status: String::new(),
        };
        if auto && !app.input.trim().is_empty() {
            app.start_translate(&cc.egui_ctx);
        }
        app
    }

    fn start_translate(&mut self, ctx: &egui::Context) {
        let text = self.input.clone();
        if text.trim().is_empty() {
            return;
        }
        let cfg = self.cfg.clone();
        let (tx, rx): (Sender<TranslationMessage>, Receiver<TranslationMessage>) =
            std::sync::mpsc::channel();
        self.pending = Some(rx);
        self.status = "Translating…".to_string();
        self.result.clear();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = translate::translate_with_warning(&cfg, &text).map_err(|e| e.to_string());
            let _ = tx.send(r);
            ctx.request_repaint();
        });
    }

    fn start_copy(&mut self, ctx: &egui::Context, text: String, success_status: String) {
        let (tx, rx): (Sender<ClipboardMessage>, Receiver<ClipboardMessage>) =
            std::sync::mpsc::channel();
        self.clipboard_pending = Some(rx);
        self.clipboard_success_status = success_status;
        self.status = "Copying to clipboard…".to_string();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = capture::set_clipboard(&text).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}

impl eframe::App for TranslatorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Poll the background translation worker.
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(translation)) => {
                    let copied = self.auto_copy && !translation.text.is_empty();
                    let success_status = match translation.warning.as_deref() {
                        Some(warning) => format!("Warning: {warning} Copied to clipboard."),
                        None => "Copied to clipboard".to_string(),
                    };
                    self.result = translation.text;
                    self.pending = None;
                    if copied {
                        self.start_copy(&ctx, self.result.clone(), success_status);
                    } else {
                        self.status = translation
                            .warning
                            .map(|warning| format!("Warning: {warning}"))
                            .unwrap_or_default();
                    }
                }
                Ok(Err(e)) => {
                    self.status = format!("Error: {e}");
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(60)),
                Err(TryRecvError::Disconnected) => self.pending = None,
            }
        }

        if let Some(rx) = &self.clipboard_pending {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.status = std::mem::take(&mut self.clipboard_success_status);
                    self.clipboard_pending = None;
                }
                Ok(Err(error)) => {
                    self.status = format!("Clipboard error: {error}");
                    self.clipboard_pending = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(60)),
                Err(TryRecvError::Disconnected) => self.clipboard_pending = None,
            }
        }

        egui::TopBottomPanel::top("bar").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("From");
                ui.add(egui::TextEdit::singleline(&mut self.cfg.source_lang).desired_width(56.0));
                ui.label("→  To");
                ui.add(egui::TextEdit::singleline(&mut self.cfg.target_lang).desired_width(56.0));
                if ui.button("⇄").on_hover_text("Swap languages").clicked() {
                    std::mem::swap(&mut self.cfg.source_lang, &mut self.cfg.target_lang);
                    if self.cfg.source_lang.is_empty() {
                        self.cfg.source_lang = "auto".into();
                    }
                }
                ui.separator();
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(self.cfg.provider.clone())
                    .show_ui(ui, |ui| {
                        for p in ["mymemory", "ai", "libre", "google"] {
                            ui.selectable_value(&mut self.cfg.provider, p.to_string(), p);
                        }
                    });
                if ui
                    .button("⚙")
                    .on_hover_text("Settings (proxy, AI key, endpoints)")
                    .clicked()
                {
                    self.show_settings = !self.show_settings;
                }
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("status").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if self.pending.is_some() || self.clipboard_pending.is_some() {
                    ui.spinner();
                }
                ui.label(&self.status);
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Source text:");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("Words: {}", word_count(&self.input)));
                });
            });
            // Source in its own height-capped scroll area so a long selection
            // can't push the translation off-screen.
            let resp = egui::ScrollArea::vertical()
                .id_salt("src")
                .max_height(130.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.input)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .hint_text("Type text, or trigger from selection / OCR…"),
                    )
                })
                .inner;

            let clicked = ui
                .horizontal(|ui| {
                    let c = ui.button("Translate  (Ctrl+Enter)").clicked();
                    if ui.button("Copy result").clicked() && !self.result.is_empty() {
                        self.start_copy(
                            &ctx,
                            self.result.clone(),
                            "Copied to clipboard".to_string(),
                        );
                    }
                    c
                })
                .inner;
            let ctrl_enter = resp.has_focus()
                && ctx.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
            if clicked || ctrl_enter {
                self.start_translate(&ctx);
            }

            ui.separator();
            ui.label("Translation:");
            // Result fills the remaining space and scrolls (auto_shrink=false)
            // instead of growing the window.
            egui::ScrollArea::vertical()
                .id_salt("res")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Feed a clone so the field is selectable/copyable but read-only.
                    let mut shown = self.result.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                });
        });

        if self.show_settings {
            self.settings_window(&ctx);
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // Escape closes the settings overlay first; only then the window.
            if self.show_settings {
                self.show_settings = false;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::word_count;

    #[test]
    fn counts_whitespace_delimited_words() {
        assert_eq!(word_count("one two\nthree\tfour"), 4);
        assert_eq!(word_count("  hello,   world!  "), 2);
    }

    #[test]
    fn empty_input_has_no_words() {
        assert_eq!(word_count(" \n\t"), 0);
    }
}

impl TranslatorApp {
    /// Floating settings overlay: edit the config that's otherwise only in
    /// ~/.config/ai-translate/config.toml. Edits apply to this session
    /// immediately; **Save** persists them to disk for future launches.
    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Settings")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        let field = 300.0;

                        ui.label("Proxy URL");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.proxy_url)
                                .desired_width(field)
                                .hint_text("http://127.0.0.1:7890 · socks5://… · blank = direct"),
                        );
                        ui.end_row();

                        ui.label("AI base URL");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.ai_base_url)
                                .desired_width(field)
                                .hint_text("https://api.deepseek.com/v1"),
                        );
                        ui.end_row();

                        ui.label("AI model");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.ai_model)
                                .desired_width(field)
                                .hint_text("deepseek-chat"),
                        );
                        ui.end_row();

                        ui.label("AI key");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.ai_key)
                                .password(true)
                                .desired_width(field)
                                .hint_text("sk-…  (enables provider = ai)"),
                        );
                        ui.end_row();

                        ui.label("LibreTranslate URL");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.libre_url)
                                .desired_width(field)
                                .hint_text("https://translate.disroot.org"),
                        );
                        ui.end_row();

                        ui.label("LibreTranslate key");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cfg.libre_key)
                                .password(true)
                                .desired_width(field)
                                .hint_text("optional"),
                        );
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.status = match self.cfg.save() {
                            Ok(()) => "Settings saved".to_string(),
                            Err(e) => format!("Save failed: {e}"),
                        };
                        self.show_settings = false;
                    }
                    if ui.button("Close").clicked() {
                        self.show_settings = false;
                    }
                    ui.add_space(8.0);
                    ui.weak("changes apply now · Save writes config.toml");
                });
            });
        // Window's own [x] / titlebar close.
        if !open {
            self.show_settings = false;
        }
    }
}
