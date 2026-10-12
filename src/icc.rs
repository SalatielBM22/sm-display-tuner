//! Escrita de perfil ICC v2.4 de monitor: primárias sRGB declaradas + curva sRGB + vcgt da ramp.
//!
//! Declarar sRGB (como o "Generic RGB" fazia no Mac) evita que apps com gerenciamento de cor
//! (Chrome, Edge, Fotos) comprimam tudo para o gamut de 51% do painel — o que cortava os verdes.
//! O ajuste de verdade fica no vcgt, que o Windows carrega na GPU.
use crate::model::{bradford, mul, rgb_to_xyz, xy_to_xyz, D50, SRGB_XY};

fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn pad4(mut b: Vec<u8>) -> Vec<u8> {
    while b.len() % 4 != 0 {
        b.push(0);
    }
    b
}

fn tag_xyz(v: [f64; 3]) -> Vec<u8> {
    let mut b = b"XYZ \0\0\0\0".to_vec();
    for x in v {
        b.extend(s15(x));
    }
    b
}

fn tag_sf32(m: &[[f64; 3]; 3]) -> Vec<u8> {
    let mut b = b"sf32\0\0\0\0".to_vec();
    for r in m {
        for x in r {
            b.extend(s15(*x));
        }
    }
    b
}

fn tag_desc(text: &str) -> Vec<u8> {
    let ascii: Vec<u8> = text.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).chain(Some(0)).collect();
    let mut b = b"desc\0\0\0\0".to_vec();
    b.extend((ascii.len() as u32).to_be_bytes());
    b.extend(&ascii);
    b.extend([0u8; 8]); // unicode lang + count
    b.extend([0u8; 3]); // scriptcode code + count
    b.extend([0u8; 67]);
    pad4(b)
}

fn tag_text(text: &str) -> Vec<u8> {
    let mut b = b"text\0\0\0\0".to_vec();
    b.extend(text.bytes().filter(|c| c.is_ascii()));
    b.push(0);
    pad4(b)
}

fn tag_curv_srgb(n: usize) -> Vec<u8> {
    let mut b = b"curv\0\0\0\0".to_vec();
    b.extend((n as u32).to_be_bytes());
    for i in 0..n {
        let v = i as f64 / (n - 1) as f64;
        let l = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        b.extend(((l * 65535.0).round() as u16).to_be_bytes());
    }
    pad4(b)
}

fn tag_vcgt(r: &[[u16; 256]; 3]) -> Vec<u8> {
    let mut b = b"vcgt\0\0\0\0".to_vec();
    b.extend(0u32.to_be_bytes()); // tipo tabela
    b.extend(3u16.to_be_bytes());
    b.extend(256u16.to_be_bytes());
    b.extend(2u16.to_be_bytes());
    for ch in r {
        for v in ch {
            b.extend(v.to_be_bytes());
        }
    }
    pad4(b)
}

pub fn build_profile(desc: &str, ramp: &[[u16; 256]; 3]) -> Vec<u8> {
    let m = rgb_to_xyz(&SRGB_XY);
    let w = xy_to_xyz(SRGB_XY[3].0, SRGB_XY[3].1);
    let chad = bradford(&w, &D50);
    let md50 = mul(&chad, &m);
    let trc = tag_curv_srgb(1024);
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", tag_desc(desc)),
        (b"cprt", tag_text("SM Display Tuner - SM Acoustics. Uso livre.")),
        (b"wtpt", tag_xyz(w)),
        (b"chad", tag_sf32(&chad)),
        (b"rXYZ", tag_xyz([md50[0][0], md50[1][0], md50[2][0]])),
        (b"gXYZ", tag_xyz([md50[0][1], md50[1][1], md50[2][1]])),
        (b"bXYZ", tag_xyz([md50[0][2], md50[1][2], md50[2][2]])),
        (b"rTRC", trc.clone()),
        (b"gTRC", trc.clone()),
        (b"bTRC", trc),
        (b"vcgt", tag_vcgt(ramp)),
    ];

    let n = tags.len();
    let mut offset = 128 + 4 + 12 * n;
    let mut table = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let mut seen: Vec<(Vec<u8>, usize)> = Vec::new();
    for (sig, blob) in &tags {
        let off = if let Some((_, o)) = seen.iter().find(|(b, _)| b == blob) {
            *o
        } else {
            let o = offset;
            data.extend(blob);
            offset += blob.len();
            seen.push((blob.clone(), o));
            o
        };
        table.extend(*sig);
        table.extend((off as u32).to_be_bytes());
        table.extend((blob.len() as u32).to_be_bytes());
    }

    let size = 128 + 4 + table.len() + data.len();
    let mut h = vec![0u8; 128];
    h[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    h[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    h[12..16].copy_from_slice(b"mntr");
    h[16..20].copy_from_slice(b"RGB ");
    h[20..24].copy_from_slice(b"XYZ ");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, mo, d, hh, mi, ss) = civil(now);
    for (k, v) in [y, mo, d, hh, mi, ss].iter().enumerate() {
        h[24 + 2 * k..26 + 2 * k].copy_from_slice(&(*v as u16).to_be_bytes());
    }
    h[36..40].copy_from_slice(b"acsp");
    h[40..44].copy_from_slice(b"MSFT");
    for (k, x) in D50.iter().enumerate() {
        h[68 + 4 * k..72 + 4 * k].copy_from_slice(&s15(*x));
    }
    h[80..84].copy_from_slice(b"SMAc");

    let mut out = h;
    out.extend((n as u32).to_be_bytes());
    out.extend(table);
    out.extend(data);
    debug_assert_eq!(out.len(), size);
    out
}

/// Segundos Unix -> (ano, mês, dia, h, m, s) UTC.
fn civil(t: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = (t / 86400) as i64;
    let rem = t % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y as u64, m as u64, d as u64, rem / 3600, rem % 3600 / 60, rem % 60)
}
