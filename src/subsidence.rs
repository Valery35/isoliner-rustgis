// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Деформации земной поверхности по оседаниям.
//!
//! Порт `grid_isolines/subsidence.py` (раздел 4 Указаний по охране
//! подрабатываемых объектов). Ядро без QGIS.

const BETA_TILT: f64 = 0.1414;
const BETA_CURV: f64 = 0.1768;
const BETA_EPS: f64 = 0.3535;
const BETA_E: f64 = 0.05;
const BETA_OE: f64 = 1.8;
const ALPHA_OE: f64 = 3.8;
const K_REF: f64 = 1e-4;
const DELTA_PERMANENT: f64 = 55.0;
const DELTA_TEMPORARY: f64 = 65.0;
const PSI_FULL: f64 = 55.0;

/// Коэффициент приведения: 1 + β·√(l/15 - 1), обрезан [15; 45].
fn q(length: f64, beta: f64) -> f64 {
    let ll = length.clamp(15.0, 45.0);
    1.0 + beta * (ll / 15.0 - 1.0).sqrt()
}

/// qᵢ для наклона.
pub fn q_tilt(length: f64) -> f64 {
    q(length, BETA_TILT)
}

/// qₖ для кривизны.
pub fn q_curv(length: f64) -> f64 {
    q(length, BETA_CURV)
}

/// q_ε для деформаций.
pub fn q_eps(length: f64) -> f64 {
    q(length, BETA_EPS)
}

/// Коэффициент mₑ перехода от кривизны к горизонтальной деформации.
pub fn m_e(curv: f64) -> f64 {
    let k = curv.abs();
    if !k.is_finite() {
        return f64::NAN;
    }
    let lam = k / K_REF;
    let bk = if lam < 1.0 {
        1.0 + BETA_OE * (1.0 - lam).powf(ALPHA_OE)
    } else {
        1.0
    };
    BETA_E * bk
}

/// ε = mₑ·K·L (п. 4.34), знак от кривизны.
pub fn eps_from_curvature(curv: f64, length: f64) -> f64 {
    if !curv.is_finite() || length <= 0.0 {
        return f64::NAN;
    }
    m_e(curv) * curv * length
}

/// Длина полумульды L = (ctg δ₀ + ctg ψ)·H.
pub fn half_trough_length(depth: f64, delta: f64, psi: f64) -> f64 {
    (1.0 / delta.to_radians().tan() + 1.0 / psi.to_radians().tan()) * depth
}

/// Удобные константы углов.
pub const ANGLE_PERMANENT: f64 = DELTA_PERMANENT;
pub const ANGLE_TEMPORARY: f64 = DELTA_TEMPORARY;
pub const ANGLE_PSI: f64 = PSI_FULL;

/// Билинейная выборка в дробных индексах. Центры ячеек — целые индексы.
fn bilinear(z: &[f64], width: usize, height: usize, fr: f64, fc: f64) -> f64 {
    if !(fr >= 0.0 && fr <= (height - 1) as f64 && fc >= 0.0 && fc <= (width - 1) as f64) {
        return f64::NAN;
    }
    if height < 2 || width < 2 {
        return f64::NAN;
    }
    let r0 = (fr.floor() as usize).min(height - 2);
    let c0 = (fc.floor() as usize).min(width - 2);
    let a = fr - r0 as f64;
    let b = fc - c0 as f64;
    let z00 = z[r0 * width + c0];
    let z01 = z[r0 * width + (c0 + 1)];
    let z10 = z[(r0 + 1) * width + c0];
    let z11 = z[(r0 + 1) * width + (c0 + 1)];
    if !z00.is_finite() || !z01.is_finite() || !z10.is_finite() || !z11.is_finite() {
        return f64::NAN;
    }
    z00 * (1.0 - a) * (1.0 - b)
        + z01 * (1.0 - a) * b
        + z10 * a * (1.0 - b)
        + z11 * a * b
}

/// Результат деформаций по гриду.
#[derive(Debug, Clone)]
pub struct Deformations {
    pub tilt: Vec<f64>,
    pub azimuth: Vec<f64>,
    pub k_dir: Vec<f64>,
    pub k_max: Vec<f64>,
    pub k_min: Vec<f64>,
}

/// Наклон и кривизна по гриду оседаний на базе `base` метров.
/// `z` — строки сверху вниз (север наверху), оседания положительны вниз, м.
pub fn grid_deformations(z: &[f64], width: usize, height: usize, cell: f64, base: f64, reduce: bool) -> Deformations {
    let n = width * height;
    let mut tilt = vec![f64::NAN; n];
    let mut azimuth = vec![f64::NAN; n];
    let mut k_dir = vec![f64::NAN; n];
    let mut k_max = vec![f64::NAN; n];
    let mut k_min = vec![f64::NAN; n];
    if width < 3 || height < 3 || cell <= 0.0 || base <= 0.0 {
        return Deformations {
            tilt,
            azimuth,
            k_dir,
            k_max,
            k_min,
        };
    }
    let h = base / cell;
    let hh = h / 2.0;
    let qi = if reduce { q_tilt(base) } else { 1.0 };
    let qk = if reduce { q_curv(base) } else { 1.0 };
    for row in 0..height {
        for col in 0..width {
            let idx = row * width + col;
            let c0 = z[idx];
            if !c0.is_finite() {
                continue;
            }
            let e_p = bilinear(z, width, height, row as f64, col as f64 + hh);
            let e_m = bilinear(z, width, height, row as f64, col as f64 - hh);
            let n_p = bilinear(z, width, height, row as f64 - hh, col as f64);
            let n_m = bilinear(z, width, height, row as f64 + hh, col as f64);
            if !e_p.is_finite() || !e_m.is_finite() || !n_p.is_finite() || !n_m.is_finite() {
                continue;
            }
            let gx = (e_p - e_m) / base;
            let gy = (n_p - n_m) / base;
            let t = (gx * gx + gy * gy).sqrt() * qi;
            tilt[idx] = t;
            if t > 1e-12 {
                let a = gx.atan2(gy).to_degrees();
                azimuth[idx] = if a < 0.0 { a + 360.0 } else { a };
            }

            let exx = (bilinear(z, width, height, row as f64, col as f64 + h)
                - 2.0 * c0
                + bilinear(z, width, height, row as f64, col as f64 - h))
                / (base * base);
            let eyy = (bilinear(z, width, height, row as f64 - h, col as f64)
                - 2.0 * c0
                + bilinear(z, width, height, row as f64 + h, col as f64))
                / (base * base);
            let exy = (bilinear(z, width, height, row as f64 - hh, col as f64 + hh)
                - bilinear(z, width, height, row as f64 + hh, col as f64 + hh)
                - bilinear(z, width, height, row as f64 - hh, col as f64 - hh)
                + bilinear(z, width, height, row as f64 + hh, col as f64 - hh))
                / (base * base);
            if !exx.is_finite() || !eyy.is_finite() || !exy.is_finite() {
                continue;
            }
            let exx = exx * qi * qk;
            let eyy = eyy * qi * qk;
            let exy = exy * qi * qk;
            let norm = (gx * gx + gy * gy).sqrt();
            let (ux, uy, tr) = if norm > 0.0 {
                (gx / norm, gy / norm, 0.5 * (exx + eyy))
            } else {
                (f64::NAN, f64::NAN, 0.5 * (exx + eyy))
            };
            let kd = if ux.is_finite() {
                exx * ux * ux + 2.0 * exy * ux * uy + eyy * uy * uy
            } else {
                tr
            };
            k_dir[idx] = kd;
            let disc = (0.25 * (exx - eyy).powi(2) + exy * exy).max(0.0).sqrt();
            k_max[idx] = tr + disc;
            k_min[idx] = tr - disc;
        }
    }
    Deformations {
        tilt,
        azimuth,
        k_dir,
        k_max,
        k_min,
    }
}
