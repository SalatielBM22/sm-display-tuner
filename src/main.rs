//! SM Display Tuner — ajuste ao vivo da curva de cor da GPU (como o BetterDisplay faz no macOS),
//! padrões de teste sem gerenciamento de cor, medição de gama por canal e exportação para ICC.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icc;
mod model;
mod patterns;
mod win;

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};
use model::Params;
use patterns::{Patterns, Tab};
use win::Ramp;

fn base_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    Path::new(&home).join(r"Documents\SM_ICC_AUO2E3C\tuner")
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let t = t.trim_matches('_').to_string();
    if t.is_empty() { "ajuste".into() } else { t }
}

struct App {
    p: Params,
    live: bool,
    keep_on_exit: bool,
    original: Option<Ramp>,
    last_applied: Option<Ramp>,
    status: (bool, String),
    patterns: Patterns,
    presets: Vec<String>,
    target: Option<win::Target>,
    current_profile: String,
    unlocked: bool,
    log: Vec<String>,
}

impl App {
    fn new() -> Self {
        let dir = base_dir();
        let _ = std::fs::create_dir_all(dir.join("presets"));
        let p = std::fs::read_to_string(dir.join("ultimo.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(Params::mac_preset);
        let target = win::primary_target().ok();
        let mut app = Self {
            p,
            live: true,
            keep_on_exit: false,
            original: win::read_ramp().ok(),
            last_applied: None,
            status: (true, String::new()),
            patterns: Patterns::default(),
            presets: Vec::new(),
            target,
            current_profile: target.and_then(win::default_profile).unwrap_or_else(|| "?".into()),
            unlocked: win::gamma_range_unlocked(),
            log: Vec::new(),
        };
        app.refresh_presets();
        app
    }

    fn note(&mut self, ok: bool, msg: impl Into<String>) {
        let m = msg.into();
        self.log.push(format!("{} {}", if ok { "✔" } else { "✖" }, m));
        if self.log.len() > 8 {
            self.log.remove(0);
        }
        self.status = (ok, m);
    }

    fn refresh_presets(&mut self) {
        self.presets = std::fs::read_dir(base_dir().join("presets"))
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        self.presets.sort();
    }

    fn desired_ramp(&self) -> Ramp {
        if self.patterns.tab == Tab::Measure {
            win::identity_ramp()
        } else {
            self.p.ramp()
        }
    }

    fn apply(&mut self) {
        if !self.live {
            return;
        }
        let r = self.desired_ramp();
        if self.last_applied.as_ref() == Some(&r) {
            return;
        }
        match win::write_ramp(&r) {
            Ok(()) => {
                self.status = (true, "curva aplicada na GPU".into());
            }
            Err(e) => {
                let hint = if self.unlocked { "" } else { " — use 'Liberar faixa completa'" };
                self.status = (false, format!("{e}{hint}"));
            }
        }
        self.last_applied = Some(r);
    }

    fn export_icc(&mut self) -> Option<PathBuf> {
        let name = format!("SM_Tuner_{}.icc", sanitize(&self.p.name));
        let path = base_dir().join(&name);
        let bytes = icc::build_profile(&format!("SM Tuner {}", self.p.name), &self.p.ramp());
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                self.note(true, format!("ICC exportado: {}", path.display()));
                Some(path)
            }
            Err(e) => {
                self.note(false, format!("falha ao gravar ICC: {e}"));
                None
            }
        }
    }

    fn activate(&mut self) {
        let Some(src) = self.export_icc() else { return };
        let Some(t) = self.target else {
            self.note(false, "tela não identificada");
            return;
        };
        let fname = src.file_name().unwrap().to_string_lossy().to_string();
        let dest = win::color_dir().join(&fname);
        let same = std::fs::read(&dest).ok() == std::fs::read(&src).ok();
        if !same {
            if let Err(e) = win::run_elevated(&["--elevated-install", &src.display().to_string()]) {
                self.note(false, format!("instalação: {e}"));
                return;
            }
            if std::fs::read(&dest).ok() != std::fs::read(&src).ok() {
                self.note(false, "o perfil não chegou na pasta de cores (UAC negado?)");
                return;
            }
        }
        let backup = base_dir().join("perfil_anterior.txt");
        if !backup.exists() && !self.current_profile.starts_with("SM_Tuner_") && self.current_profile != "?" {
            let _ = std::fs::write(&backup, &self.current_profile);
        }
        match win::set_default_profile(t, &fname) {
            Ok(()) => {
                self.current_profile = fname.clone();
                win::reload_calibration();
                self.keep_on_exit = true;
                self.note(true, format!("{fname} ativo (carrega sozinho no login)"));
            }
            Err(e) => self.note(false, e),
        }
    }

    fn restore_previous(&mut self) {
        let Some(t) = self.target else { return };
        let Ok(prev) = std::fs::read_to_string(base_dir().join("perfil_anterior.txt")) else {
            self.note(false, "sem registro do perfil anterior");
            return;
        };
        match win::set_default_profile(t, prev.trim()) {
            Ok(()) => {
                win::reload_calibration();
                self.current_profile = prev.trim().to_string();
                self.live = false;
                self.keep_on_exit = true;
                self.note(true, format!("voltou para {} (ajuste ao vivo pausado)", prev.trim()));
            }
            Err(e) => self.note(false, e),
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("SM Display Tuner");
        ui.label(format!("Painel: {}   Perfil ativo: {}", model::PANEL_NAME, self.current_profile));
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.live, "Ao vivo").changed() && self.live {
                self.last_applied = None;
            }
            if ui.button("Reaplicar").clicked() {
                self.last_applied = None;
            }
            if ui.button("Neutro na GPU").on_hover_text("curva identidade (desliga o ajuste para comparar)").clicked() {
                let _ = win::write_ramp(&win::identity_ramp());
                self.live = false;
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Nome:");
            ui.text_edit_singleline(&mut self.p.name);
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("presets").selected_text("Carregar preset…").show_ui(ui, |ui| {
                if ui.selectable_label(false, "Neutro").clicked() {
                    self.p = Params::default();
                }
                if ui.selectable_label(false, "Mac (BetterDisplay, aprox.)").clicked() {
                    self.p = Params::mac_preset();
                }
                for n in self.presets.clone() {
                    if ui.selectable_label(false, &n).clicked() {
                        if let Some(p) = std::fs::read_to_string(base_dir().join("presets").join(format!("{n}.json")))
                            .ok()
                            .and_then(|s| serde_json::from_str::<Params>(&s).ok())
                        {
                            let measured = self.p.measured;
                            self.p = p;
                            if self.p.measured == [2.2; 3] {
                                self.p.measured = measured;
                            }
                        }
                    }
                }
            });
            if ui.button("Salvar preset").clicked() {
                let path = base_dir().join("presets").join(format!("{}.json", sanitize(&self.p.name)));
                let ok = std::fs::write(&path, serde_json::to_string_pretty(&self.p).unwrap()).is_ok();
                self.note(ok, format!("preset {}", path.display()));
                self.refresh_presets();
            }
        });
        ui.separator();

        ui.label(RichText::new("Global").strong());
        ui.add(egui::Slider::new(&mut self.p.brightness, 0.5..=1.0).text("Brilho"));
        ui.add(egui::Slider::new(&mut self.p.target_gamma, 1.6..=2.8).step_by(0.01).text("Gama alvo"));
        ui.add(egui::Slider::new(&mut self.p.contrast, -0.5..=0.5).step_by(0.01).text("Contraste"));
        ui.add(egui::Slider::new(&mut self.p.black, 0.0..=0.10).step_by(0.002).text("Preto"));
        ui.checkbox(&mut self.p.temp_enabled, "Temperatura de cor (pelas primárias do EDID)");
        ui.add_enabled(self.p.temp_enabled, egui::Slider::new(&mut self.p.temp_k, 4000.0..=9000.0).step_by(50.0).text("K"));
        ui.separator();

        for (c, name) in ["Vermelho", "Verde", "Azul"].iter().enumerate() {
            egui::CollapsingHeader::new(RichText::new(*name).strong()).id_salt(c).default_open(true).show(ui, |ui| {
                ui.add(egui::Slider::new(&mut self.p.ch[c].gain, 0.5..=1.0).step_by(0.001).text("Ganho"));
                ui.add(egui::Slider::new(&mut self.p.ch[c].gamma, 0.7..=1.4).step_by(0.005).text("Gama"));
                ui.add(egui::Slider::new(&mut self.p.ch[c].black, 0.0..=0.08).step_by(0.001).text("Preto"));
            });
        }
        ui.horizontal(|ui| {
            ui.label("Gama medido R/G/B:");
            for c in 0..3 {
                ui.add(egui::DragValue::new(&mut self.p.measured[c]).range(1.4..=3.2).speed(0.01).fixed_decimals(2));
            }
        });
        ui.separator();

        ui.label(RichText::new("Salvar no Windows").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Exportar ICC").clicked() {
                self.export_icc();
            }
            if ui.button("Instalar e ativar").on_hover_text("pede UAC só na 1ª vez de cada arquivo").clicked() {
                self.activate();
            }
            if ui.button("Voltar ao perfil anterior").clicked() {
                self.restore_previous();
            }
        });
        if !self.unlocked {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Faixa de curva limitada pelo Windows.").color(Color32::YELLOW));
                if ui.button("Liberar faixa completa (admin)").clicked() {
                    match win::run_elevated(&["--elevated-unlock"]) {
                        Ok(()) => {
                            self.unlocked = win::gamma_range_unlocked();
                            self.last_applied = None;
                            self.note(self.unlocked, "GdiIcmGammaRange = 256");
                        }
                        Err(e) => self.note(false, e),
                    }
                }
            });
        }
        ui.checkbox(&mut self.keep_on_exit, "Manter a curva ao fechar");
        ui.separator();
        ui.small(
            "ACM ('Gerenciar cor automaticamente') precisa estar DESLIGADO. \
             Saturação: use a do AMD (Matiz 0, Contraste 100); este app ajusta brilho, gama, preto e balanço por canal.",
        );
        for l in &self.log {
            ui.small(l);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let (ok, msg) = &self.status;
            ui.label(RichText::new(msg).color(if *ok { Color32::LIGHT_GREEN } else { Color32::LIGHT_RED }));
        });
        egui::SidePanel::left("controls").resizable(true).default_width(360.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
        });
        let mut measured = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_gray(48)).inner_margin(10.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    measured = self.patterns.ui(ui);
                });
            });
        if let Some((ch, g)) = measured {
            if ch == 3 {
                self.p.measured = [g; 3];
            } else {
                self.p.measured[ch] = g;
            }
            self.note(true, format!("gama medido aplicado: {g:.2}"));
        }
        self.apply();
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::write(base_dir().join("ultimo.json"), serde_json::to_string_pretty(&self.p).unwrap_or_default());
        if !self.keep_on_exit {
            if let Some(o) = self.original {
                let _ = win::write_ramp(&o);
            }
        }
    }
}

/// Modos sem interface: usados pelo processo elevado e para autoteste.
fn headless(args: &[String]) -> Option<i32> {
    match args.get(1).map(String::as_str) {
        Some("--elevated-install") => {
            let src = PathBuf::from(args.get(2)?);
            let dest = win::color_dir().join(src.file_name()?);
            Some(if std::fs::copy(&src, &dest).is_ok() { 0 } else { 1 })
        }
        Some("--elevated-unlock") => Some(if win::unlock_gamma_range() { 0 } else { 1 }),
        Some("--selftest") => Some(selftest()),
        _ => None,
    }
}

fn selftest() -> i32 {
    let mut out = String::new();
    let mut fails = 0;
    let mut check = |ok: bool, what: String, out: &mut String| {
        out.push_str(&format!("{} {}\n", if ok { "OK  " } else { "FAIL" }, what));
        if !ok {
            fails += 1;
        }
    };
    let orig = win::read_ramp();
    check(orig.is_ok(), format!("read_ramp {:?}", orig.as_ref().err()), &mut out);
    if let Ok(o) = &orig {
        out.push_str(&format!("     ramp atual R[0,64,128,255] = {:?}\n", [o[0][0], o[0][64], o[0][128], o[0][255]]));
        let test = Params::mac_preset().ramp();
        let w = win::write_ramp(&test);
        check(w.is_ok(), format!("write_ramp(preset Mac) {:?}", w.err()), &mut out);
        let back = win::read_ramp();
        let diff = back.as_ref().map(|b| (0..3).flat_map(|c| (0..256).map(move |i| (c, i))).map(|(c, i)| (b[c][i] as i32 - test[c][i] as i32).abs()).max().unwrap_or(0)).unwrap_or(-1);
        check(diff >= 0 && diff <= 256, format!("leitura de volta, diferença máx = {diff}"), &mut out);
        let r = win::write_ramp(o);
        check(r.is_ok(), "ramp original restaurada".into(), &mut out);
    }
    let t = win::primary_target();
    check(t.is_ok(), format!("primary_target {:?}", t), &mut out);
    if let Ok(t) = t {
        out.push_str(&format!("     perfil padrão: {:?}\n", win::default_profile(t)));
    }
    out.push_str(&format!("     GdiIcmGammaRange liberado: {}\n", win::gamma_range_unlocked()));
    let icc = icc::build_profile("SM Tuner selftest", &Params::mac_preset().ramp());
    let p = base_dir().join("selftest.icc");
    let _ = std::fs::create_dir_all(base_dir());
    check(std::fs::write(&p, &icc).is_ok(), format!("ICC gravado ({} bytes) em {}", icc.len(), p.display()), &mut out);
    let _ = std::fs::write(base_dir().join("selftest.txt"), &out);
    fails
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = headless(&args) {
        std::process::exit(code);
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("SM Display Tuner").with_inner_size([1280.0, 720.0]).with_maximized(true),
        ..Default::default()
    };
    eframe::run_native("SM Display Tuner", opts, Box::new(|_cc| Ok(Box::new(App::new()))))
}
