//! Modelo do ajuste: parâmetros -> curva (ramp) 3x256 de 16 bits.
//!
//! Pipeline por canal c, entrada v em [0,1] (código):
//!   1. contraste S (extremos fixos, monotônico para |k| < 1)
//!   2. expoente e_c = (gama_alvo / gama_medido_c) * gama_usuario_c
//!   3. preto: b + (1-b)*v
//!   4. ganho: * ganho_c * brilho * ganho_temperatura_c
use serde::{Deserialize, Serialize};

pub type Mat3 = [[f64; 3]; 3];

/// Primárias e branco declarados no EDID do painel AUO 2E3C (lidos do registro).
pub const PANEL_NAME: &str = "AUO 2E3C (EDID)";
pub const PANEL_XY: [(f64, f64); 4] = [
    (0.580_078, 0.334_961),
    (0.330_078, 0.565_430),
    (0.155_273, 0.139_648),
    (0.313_477, 0.329_102),
];
pub const SRGB_XY: [(f64, f64); 4] = [(0.64, 0.33), (0.30, 0.60), (0.15, 0.06), (0.3127, 0.3290)];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    pub gain: f32,
    pub gamma: f32,
    pub black: f32,
}
impl Default for Channel {
    fn default() -> Self {
        Self { gain: 1.0, gamma: 1.0, black: 0.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Params {
    pub name: String,
    pub brightness: f32,
    pub target_gamma: f32,
    pub contrast: f32,
    pub black: f32,
    pub temp_enabled: bool,
    pub temp_k: f32,
    pub ch: [Channel; 3],
    /// Gama real de cada canal do painel (medido no olho); 2.2 = valor do EDID.
    pub measured: [f32; 3],
}
impl Default for Params {
    fn default() -> Self {
        Self {
            name: "Neutro".into(),
            brightness: 1.0,
            target_gamma: 2.2,
            contrast: 0.0,
            black: 0.0,
            temp_enabled: false,
            temp_k: 6500.0,
            ch: [Channel::default(); 3],
            measured: [2.2; 3],
        }
    }
}

impl Params {
    /// Ponto de partida a partir do betterdisplay_v2.plist (Display:2 = este painel).
    /// Ganhos BD (-1..+1): R -0.359, G -0.379, B -0.417 -> (1+v) normalizado pelo vermelho.
    pub fn mac_preset() -> Self {
        let raw = [1.0 - 0.359_12_f32, 1.0 - 0.379_43, 1.0 - 0.417_21];
        let mut p = Self { name: "Mac (BetterDisplay, aprox.)".into(), ..Self::default() };
        for c in 0..3 {
            p.ch[c].gain = raw[c] / raw[0];
        }
        p
    }

    pub fn ramp(&self) -> [[u16; 256]; 3] {
        let tg = if self.temp_enabled { temperature_gains(self.temp_k as f64, &self.measured) } else { [1.0; 3] };
        let mut out = [[0u16; 256]; 3];
        for c in 0..3 {
            let ch = self.ch[c];
            let e = (self.target_gamma / self.measured[c].max(0.5)) * ch.gamma;
            let b = (self.black + ch.black).clamp(0.0, 0.5);
            let g = ch.gain * self.brightness * tg[c] as f32;
            for (i, o) in out[c].iter_mut().enumerate() {
                let v = i as f32 / 255.0;
                let v = contrast(v, self.contrast).clamp(0.0, 1.0);
                let v = v.powf(e);
                let v = b + (1.0 - b) * v;
                let v = (v * g).clamp(0.0, 1.0);
                *o = (v * 65535.0).round() as u16;
            }
        }
        out
    }
}

fn contrast(v: f32, k: f32) -> f32 {
    use std::f32::consts::TAU;
    v - k * (TAU * v).sin() / TAU
}

// ---------------------------------------------------------------------------
// Colorimetria
// ---------------------------------------------------------------------------
pub fn xy_to_xyz(x: f64, y: f64) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

pub fn mulv(a: &Mat3, v: &[f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i][0] * v[0] + a[i][1] * v[1] + a[i][2] * v[2])
}

pub fn inv(m: &Mat3) -> Mat3 {
    let d = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let mut r = [[0.0; 3]; 3];
    r[0][0] = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / d;
    r[0][1] = (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / d;
    r[0][2] = (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / d;
    r[1][0] = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / d;
    r[1][1] = (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / d;
    r[1][2] = (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / d;
    r[2][0] = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / d;
    r[2][1] = (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / d;
    r[2][2] = (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / d;
    r
}

/// Matriz RGB(linear) -> XYZ com Y(branco)=1.
pub fn rgb_to_xyz(p: &[(f64, f64); 4]) -> Mat3 {
    let cols = [xy_to_xyz(p[0].0, p[0].1), xy_to_xyz(p[1].0, p[1].1), xy_to_xyz(p[2].0, p[2].1)];
    let pm: Mat3 = [0, 1, 2].map(|i| [cols[0][i], cols[1][i], cols[2][i]]);
    let w = xy_to_xyz(p[3].0, p[3].1);
    let s = mulv(&inv(&pm), &w);
    [0, 1, 2].map(|i| [pm[i][0] * s[0], pm[i][1] * s[1], pm[i][2] * s[2]])
}

pub const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

pub fn bradford(src: &[f64; 3], dst: &[f64; 3]) -> Mat3 {
    let b: Mat3 = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];
    let cs = mulv(&b, src);
    let cd = mulv(&b, dst);
    let d: Mat3 = [[cd[0] / cs[0], 0.0, 0.0], [0.0, cd[1] / cs[1], 0.0], [0.0, 0.0, cd[2] / cs[2]]];
    mul(&inv(&b), &mul(&d, &b))
}

/// CCT -> xy (locus de luz do dia CIE, 4000–25000 K).
pub fn cct_to_xy(t: f64) -> (f64, f64) {
    let t = t.clamp(4000.0, 25000.0);
    let x = if t <= 7000.0 {
        -4.6070e9 / t.powi(3) + 2.9678e6 / t.powi(2) + 0.09911e3 / t + 0.244063
    } else {
        -2.0064e9 / t.powi(3) + 1.9018e6 / t.powi(2) + 0.24748e3 / t + 0.237040
    };
    (x, -3.0 * x * x + 2.870 * x - 0.275)
}

/// Ganhos de código por canal para o branco do painel cair em `t` K.
pub fn temperature_gains(t: f64, measured: &[f32; 3]) -> [f64; 3] {
    let m = rgb_to_xyz(&PANEL_XY);
    let (x, y) = cct_to_xy(t);
    let lin = mulv(&inv(&m), &xy_to_xyz(x, y));
    let mx = lin.iter().cloned().fold(f64::MIN, f64::max);
    [0, 1, 2].map(|c| (lin[c].max(1e-6) / mx).powf(1.0 / measured[c].max(0.5) as f64))
}

/// Gama de um canal a partir do valor (0..255) que casa com um padrão de linhas de `fraction` de luminância.
pub fn gamma_from_match(value: u8, fraction: f64) -> Option<f64> {
    let v = value as f64 / 255.0;
    if !(0.02..0.98).contains(&v) {
        return None;
    }
    Some(fraction.ln() / v.ln())
}
