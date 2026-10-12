//! Padrões de teste desenhados sem gerenciamento de cor (valores de código exatos).
use eframe::egui::{self, pos2, Align2, Color32, ColorImage, FontId, Rect, Sense, TextureHandle, TextureOptions, Vec2};

use crate::model::gamma_from_match;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Gradients,
    Ends,
    Measure,
    Colors,
}

const CH_NAMES: [&str; 4] = ["Vermelho", "Verde", "Azul", "Cinza"];

fn ch_color(ch: usize, v: u8) -> Color32 {
    match ch {
        0 => Color32::from_rgb(v, 0, 0),
        1 => Color32::from_rgb(0, v, 0),
        2 => Color32::from_rgb(0, 0, v),
        _ => Color32::from_rgb(v, v, v),
    }
}

fn rgb_bytes(ch: usize, v: u8) -> [u8; 3] {
    let c = ch_color(ch, v);
    [c.r(), c.g(), c.b()]
}

pub struct Patterns {
    pub tab: Tab,
    grads: Vec<TextureHandle>,
    pub measure_ch: usize,
    pub match50: [u8; 4],
    pub match25: [u8; 4],
    lines: Option<((usize, usize, usize), [TextureHandle; 2])>,
}

impl Default for Patterns {
    fn default() -> Self {
        Self { tab: Tab::Gradients, grads: Vec::new(), measure_ch: 1, match50: [186; 4], match25: [137; 4], lines: None }
    }
}

/// Retângulo alinhado à grade de pixels físicos, com tamanho exato em pixels.
fn pixel_rect(ui: &mut egui::Ui, px: [usize; 2]) -> Rect {
    let ppp = ui.ctx().pixels_per_point();
    let size = Vec2::new(px[0] as f32 / ppp, px[1] as f32 / ppp);
    let (alloc, _) = ui.allocate_exact_size(size + Vec2::splat(2.0 / ppp), Sense::hover());
    let min = alloc.min;
    let snapped = pos2((min.x * ppp).ceil() / ppp, (min.y * ppp).ceil() / ppp);
    Rect::from_min_size(snapped, size)
}

impl Patterns {
    fn ensure_grads(&mut self, ctx: &egui::Context) {
        if !self.grads.is_empty() {
            return;
        }
        for ch in 0..4 {
            let smooth: Vec<u8> = (0..=255u8).flat_map(|v| rgb_bytes(ch, v)).collect();
            self.grads.push(ctx.load_texture(format!("grad{ch}"), ColorImage::from_rgb([256, 1], &smooth), TextureOptions::NEAREST));
            let steps: Vec<u8> = (0..32u32).flat_map(|i| rgb_bytes(ch, ((i * 255) / 31) as u8)).collect();
            self.grads.push(ctx.load_texture(format!("step{ch}"), ColorImage::from_rgb([32, 1], &steps), TextureOptions::NEAREST));
        }
    }

    fn ensure_lines(&mut self, ctx: &egui::Context, w: usize, h: usize, ch: usize) {
        if matches!(&self.lines, Some((k, _)) if *k == (w, h, ch)) {
            return;
        }
        let make = |period: usize| -> ColorImage {
            let mut b = Vec::with_capacity(w * h * 3);
            for y in 0..h {
                let v = if y % period == 0 { 255 } else { 0 };
                for _ in 0..w {
                    b.extend(rgb_bytes(ch, v));
                }
            }
            ColorImage::from_rgb([w, h], &b)
        };
        let t50 = ctx.load_texture("lines50", make(2), TextureOptions::NEAREST);
        let t25 = ctx.load_texture("lines25", make(4), TextureOptions::NEAREST);
        self.lines = Some(((w, h, ch), [t50, t25]));
    }

    /// Desenha a aba ativa. Retorna Some((canal, gama)) quando o usuário aplica uma medição.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<(usize, f32)> {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Gradients, "Gradientes");
            ui.selectable_value(&mut self.tab, Tab::Ends, "Preto / Branco");
            ui.selectable_value(&mut self.tab, Tab::Measure, "Medir gama");
            ui.selectable_value(&mut self.tab, Tab::Colors, "Cores");
        });
        ui.separator();
        match self.tab {
            Tab::Gradients => {
                self.gradients(ui);
                None
            }
            Tab::Ends => {
                self.ends(ui);
                None
            }
            Tab::Measure => self.measure(ui),
            Tab::Colors => {
                self.colors(ui);
                None
            }
        }
    }

    fn gradients(&mut self, ui: &mut egui::Ui) {
        self.ensure_grads(ui.ctx());
        let w = ui.available_width();
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        for ch in 0..4 {
            ui.label(format!("{} — contínuo (256 níveis) e 32 degraus", CH_NAMES[ch]));
            let (r, _) = ui.allocate_exact_size(Vec2::new(w, 46.0), Sense::hover());
            ui.painter().image(self.grads[ch * 2].id(), r, full, Color32::WHITE);
            let (r, _) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::hover());
            ui.painter().image(self.grads[ch * 2 + 1].id(), r, full, Color32::WHITE);
            ui.add_space(4.0);
        }
        ui.small("Procure: degraus todos distintos, sem faixas grudadas no fim (estouro) nem no começo (preto esmagado), cinza sem tingir.");
    }

    fn ends(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let n = 21usize;
        let cell = (w / n as f32).min(48.0);
        for ch in [3usize, 0, 1, 2] {
            for (base, bg) in [(0u8, 0u8), (235u8, 255u8)] {
                let (r, _) = ui.allocate_exact_size(Vec2::new(cell * n as f32, cell + 14.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(r, 0.0, ch_color(ch, bg));
                for i in 0..n {
                    let v = base + i as u8;
                    let cr = Rect::from_min_size(pos2(r.min.x + i as f32 * cell, r.min.y), Vec2::splat(cell)).shrink(cell * 0.18);
                    p.rect_filled(cr, 0.0, ch_color(ch, v));
                    p.text(
                        pos2(cr.center().x, r.max.y - 6.0),
                        Align2::CENTER_CENTER,
                        v.to_string(),
                        FontId::proportional(9.0),
                        Color32::from_gray(140),
                    );
                }
            }
            ui.add_space(6.0);
        }
        ui.small("Linha de cima: 0–20 sobre preto 0. Linha de baixo: 235–255 sobre branco 255. Conte a partir de qual número o quadrado aparece.");
    }

    fn measure(&mut self, ui: &mut egui::Ui) -> Option<(usize, f32)> {
        let mut apply = None;
        ui.label("Enquanto esta aba está aberta a curva da GPU fica neutra, para medir o painel cru.");
        ui.horizontal(|ui| {
            for (i, n) in CH_NAMES.iter().enumerate() {
                ui.selectable_value(&mut self.measure_ch, i, *n);
            }
        });
        let ch = self.measure_ch;
        let (w, h) = (360usize, 200usize);
        self.ensure_lines(ui.ctx(), w, h, ch);
        let tex = self.lines.as_ref().map(|(_, t)| [t[0].id(), t[1].id()]).unwrap();
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let ppp = ui.ctx().pixels_per_point();

        let mut gammas = [None, None];
        ui.horizontal(|ui| {
            for (k, (frac, label)) in [(0.5, "50% (1 linha acesa a cada 2)"), (0.25, "25% (1 linha acesa a cada 4)")].iter().enumerate() {
                ui.vertical(|ui| {
                    ui.label(*label);
                    let r = pixel_rect(ui, [w, h]);
                    ui.painter().image(tex[k], r, full, Color32::WHITE);
                    let pw = (w / 3) as f32 / ppp;
                    let ph = (h / 3) as f32 / ppp;
                    let patch = Rect::from_center_size(r.center(), Vec2::new(pw, ph));
                    let patch = Rect::from_min_size(pos2((patch.min.x * ppp).round() / ppp, (patch.min.y * ppp).round() / ppp), patch.size());
                    let m = if k == 0 { &mut self.match50[ch] } else { &mut self.match25[ch] };
                    ui.painter().rect_filled(patch, 0.0, ch_color(ch, *m));
                    ui.add(egui::Slider::new(&mut *m, 1..=254).text("valor do quadrado"));
                    let g = gamma_from_match(*m, *frac);
                    gammas[k] = g;
                    ui.label(match g {
                        Some(g) => format!("gama estimado: {g:.2}"),
                        None => "fora da faixa".into(),
                    });
                });
                ui.add_space(12.0);
            }
        });
        ui.add_space(6.0);
        ui.label(
            "Olhe de frente, na altura do centro da tela, e desfoque a vista (ou afaste-se ~2 m). \
             Ajuste até o quadrado sumir no fundo de linhas. Painel TN muda com o ângulo: mantenha a cabeça parada.",
        );
        let avg = match gammas {
            [Some(a), Some(b)] => Some((a + b) / 2.0),
            [Some(a), None] | [None, Some(a)] => Some(a),
            _ => None,
        };
        if let Some(g) = avg {
            let label = if ch == 3 { format!("Usar {g:.2} nos três canais") } else { format!("Usar {g:.2} no {}", CH_NAMES[ch]) };
            if ui.button(label).clicked() {
                apply = Some((ch, g as f32));
            }
        }
        apply
    }

    fn colors(&mut self, ui: &mut egui::Ui) {
        const CC: [(&str, [u8; 3]); 24] = [
            ("pele escura", [115, 82, 68]),
            ("pele clara", [194, 150, 130]),
            ("céu", [98, 122, 157]),
            ("folhagem", [87, 108, 67]),
            ("flor azul", [133, 128, 177]),
            ("verde-azulado", [103, 189, 170]),
            ("laranja", [214, 126, 44]),
            ("azul arroxeado", [80, 91, 166]),
            ("vermelho moderado", [193, 90, 99]),
            ("roxo", [94, 60, 108]),
            ("verde-limão", [157, 188, 64]),
            ("amarelo-laranja", [224, 163, 46]),
            ("azul", [56, 61, 150]),
            ("verde", [70, 148, 73]),
            ("vermelho", [175, 54, 60]),
            ("amarelo", [231, 199, 31]),
            ("magenta", [187, 86, 149]),
            ("ciano", [8, 133, 161]),
            ("branco", [243, 243, 242]),
            ("neutro 8", [200, 200, 200]),
            ("neutro 6.5", [160, 160, 160]),
            ("neutro 5", [122, 122, 121]),
            ("neutro 3.5", [85, 85, 85]),
            ("preto", [52, 52, 52]),
        ];
        let w = ui.available_width();
        let cw = (w / 6.0).min(120.0);
        let chh = cw * 0.62;
        let (r, _) = ui.allocate_exact_size(Vec2::new(cw * 6.0, chh * 4.0), Sense::hover());
        let p = ui.painter();
        for (i, (name, c)) in CC.iter().enumerate() {
            let (col, row) = (i % 6, i / 6);
            let cr = Rect::from_min_size(pos2(r.min.x + col as f32 * cw, r.min.y + row as f32 * chh), Vec2::new(cw, chh)).shrink(3.0);
            p.rect_filled(cr, 4.0, Color32::from_rgb(c[0], c[1], c[2]));
            let lum = 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
            let tc = if lum > 120.0 { Color32::BLACK } else { Color32::WHITE };
            p.text(pos2(cr.min.x + 4.0, cr.max.y - 4.0), Align2::LEFT_BOTTOM, *name, FontId::proportional(10.0), tc);
        }
        ui.add_space(8.0);
        let ramps: [(&str, [f32; 3]); 4] = [
            ("verde puro", [0.0, 1.0, 0.0]),
            ("verde 50% sat.", [0.5, 1.0, 0.5]),
            ("folhagem", [0.55, 0.72, 0.42]),
            ("pele", [1.0, 0.77, 0.67]),
        ];
        for (name, k) in ramps {
            ui.label(format!("{name} — 24 degraus"));
            let (r, _) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::hover());
            let sw = w / 24.0;
            for i in 0..24 {
                let v = (i as f32 + 1.0) / 24.0 * 255.0;
                let c = Color32::from_rgb((v * k[0]) as u8, (v * k[1]) as u8, (v * k[2]) as u8);
                ui.painter().rect_filled(Rect::from_min_size(pos2(r.min.x + i as f32 * sw, r.min.y), Vec2::new(sw + 0.5, 30.0)), 0.0, c);
            }
        }
        ui.small("Pele não deve puxar para verde nem rosa; os neutros não devem ter cor; cada degrau de verde deve ser distinguível.");
    }
}
