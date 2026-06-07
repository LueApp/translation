use crate::config::Config;
use crate::{capture, translate};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

pub struct TranslatorApp {
    cfg: Config,
    input: String,
    result: String,
    status: String,
    auto_copy: bool,
    pending: Option<Receiver<Result<String, String>>>,
}

impl TranslatorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        initial: String,
        auto: bool,
        auto_copy: bool,
    ) -> Self {
        // Load a CJK-capable font as a fallback so Chinese/Japanese/Korean render
        // (egui's built-in fonts have no CJK glyphs → otherwise shows □□□).
        let mut fonts = egui::FontDefinitions::default();
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
            pending: None,
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
        let (tx, rx): (Sender<Result<String, String>>, Receiver<Result<String, String>>) =
            std::sync::mpsc::channel();
        self.pending = Some(rx);
        self.status = "Translating…".to_string();
        self.result.clear();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = translate::translate(&cfg, &text).map_err(|e| e.to_string());
            let _ = tx.send(r);
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
                Ok(Ok(text)) => {
                    if self.auto_copy && !text.is_empty() {
                        let _ = capture::set_clipboard(&text);
                        self.status = "Copied to clipboard".to_string();
                    } else {
                        self.status.clear();
                    }
                    self.result = text;
                    self.pending = None;
                }
                Ok(Err(e)) => {
                    self.status = format!("Error: {e}");
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(60)),
                Err(TryRecvError::Disconnected) => self.pending = None,
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
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("status").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if self.pending.is_some() {
                    ui.spinner();
                }
                ui.label(&self.status);
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.label("Source text:");
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
                        let _ = capture::set_clipboard(&self.result);
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

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
