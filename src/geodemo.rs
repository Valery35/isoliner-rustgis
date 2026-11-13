// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Демонстрационные геофизические профили, порт `grid_isolines/geodemo.py`.
//! Два режима: электроразведка (ρк, ЕП, ВП с низкоомной аномалией-пятном)
//! и оседания (мульда сдвижения по турам). Случайные числа берутся из
//! генератора, совместимого с NumPy, в том же порядке, поэтому при
//! одном зерне данные совпадают с модулем.

use std::f64::consts::PI;

use crate::npy_rng::Rng;
use crate::numpy;

/// Деление с округлением вниз, как `//` Python для дробных чисел.
pub fn floor_div(a: f64, b: f64) -> f64 {
    let m = a % b;
    let mut div = (a - m) / b;
    if m != 0.0 && ((b < 0.0) != (m < 0.0)) {
        div -= 1.0;
    }
    if div != 0.0 {
        let mut f = div.floor();
        if div - f > 0.5 {
            f += 1.0;
        }
        f
    } else {
        0.0f64.copysign(a / b)
    }
}

/// Пикет в метрах в номер и плюс: 520 -> (5, 20), подпись «ПК5+20»
/// собирает вызывающий на своём языке.
pub fn pk_parts(m: f64) -> (i64, i64) {
    let mut n = floor_div(m, 100.0) as i64;
    let mut plus = (m - n as f64 * 100.0).round_ties_even() as i64;
    if plus >= 100 {
        n += 1;
        plus -= 100;
    }
    (n, plus)
}

/// Компактное пятно (гаусс) в точках профиля на высоте `y`.
fn blob(px: &[f64], y: f64, cx: f64, cy: f64, sx: f64, sy: f64) -> Vec<f64> {
    let dy = y - cy;
    let ty = (dy * dy) / (2.0 * sy * sy);
    px.iter()
        .map(|&x| {
            let dx = x - cx;
            (-((dx * dx) / (2.0 * sx * sx) + ty)).exp()
        })
        .collect()
}

/// Пикеты вдоль профилей.
struct Layout {
    px: Vec<f64>,
    pys: Vec<f64>,
    x0: f64,
    length: f64,
    h: f64,
    ymin: f64,
}

fn geometry(
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
    n_profiles: i64,
    picket_step: f64,
) -> Option<Layout> {
    let w = xmax - xmin;
    let h = ymax - ymin;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mx = 0.05 * w;
    let my = 0.10 * h;
    let x0 = xmin + mx;
    let length = (xmax - mx) - x0;
    let step = if picket_step > 0.0 {
        picket_step
    } else {
        (length / 40.0).max(1.0)
    };
    let npk = (floor_div(length, step) as i64 + 1).max(2);
    let px = (0..npk).map(|k| x0 + k as f64 * step).collect();
    let n_profiles = n_profiles.max(1);
    let pys = if n_profiles == 1 {
        vec![(ymin + ymax) / 2.0]
    } else {
        numpy::linspace(ymin + my, ymax - my, n_profiles as usize)
    };
    Some(Layout {
        px,
        pys,
        x0,
        length,
        h,
        ymin,
    })
}

fn relief(l: &Layout, y: f64, z_base: f64, z_amp: f64) -> Vec<f64> {
    let v = (y - l.ymin) / l.h.max(1e-9);
    let cv = (PI * (v * 1.1 + 0.2)).cos();
    l.px.iter()
        .map(|&x| {
            let u = (x - l.x0) / l.length.max(1e-9);
            z_base + z_amp * ((2.0 * PI * (u * 1.3 + 0.15)).sin() * cv)
        })
        .collect()
}

/// Точки профилей: столбцы как у словаря модуля.
#[derive(Debug, Clone, Default)]
pub struct Profiles {
    pub profile: Vec<i64>,
    pub picket_m: Vec<f64>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    pub rho_k: Vec<f64>,
    pub rho_true: Vec<f64>,
    pub sp: Vec<f64>,
    pub vp: Vec<f64>,
    pub tour: Vec<i64>,
    pub settle: Vec<f64>,
    pub settle_true: Vec<f64>,
}

impl Profiles {
    fn push_line(&mut self, ip: i64, l: &Layout, y: f64, z: Vec<f64>) {
        let n = l.px.len();
        self.profile.extend(std::iter::repeat_n(ip, n));
        self.picket_m.extend(l.px.iter().map(|x| x - l.x0));
        self.x.extend(&l.px);
        self.y.extend(std::iter::repeat_n(y, n));
        self.z.extend(z);
    }
}

/// Параметры электроразведки (`gen_profiles`).
#[derive(Debug, Clone, Copy)]
pub struct Electro {
    pub rho_bg: f64,
    pub rho_min: f64,
    pub sp_amp: f64,
    pub vp_bg: f64,
    pub vp_amp: f64,
    pub noise: f64,
}

/// Общие параметры: охват, профили, пикеты, рельеф.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub ext: (f64, f64, f64, f64),
    pub n_profiles: i64,
    pub picket_step: f64,
    pub z_base: f64,
    pub z_amp: f64,
}

/// Электроразведка. `None` - пустой охват.
pub fn gen_profiles(f: &Frame, e: &Electro, rng: &mut Rng) -> Option<Profiles> {
    let (xmin, ymin, xmax, ymax) = f.ext;
    let l = geometry(xmin, ymin, xmax, ymax, f.n_profiles, f.picket_step)?;
    let npk = l.px.len();
    let np = l.pys.len();
    // пятно на одном из внутренних профилей, смещено по X
    let ip_anom = if np < 3 {
        np / 2
    } else {
        rng.integers(1, np as i64 - 1) as usize
    };
    let cx = l.x0 + rng.uniform(0.35, 0.65) * l.length;
    let cy = l.pys[ip_anom] + rng.uniform(-0.4, 0.4) * (l.h / np.max(1) as f64);
    let sx = 0.12 * l.length;
    let sy = 0.16 * l.h;
    // лёгкий 2D-фон, чтобы профили не были синхронны
    let fx = rng.uniform(0.8, 1.6);
    let fy = rng.uniform(0.6, 1.2);
    let phx = rng.uniform(0.0, 2.0 * PI);
    let phy = rng.uniform(0.0, 2.0 * PI);
    let lr_bg = e.rho_bg.max(1e-3).log10();
    let lr_min = e.rho_min.max(1e-3).log10();
    let depth = lr_bg - lr_min;
    let mut out = Profiles::default();
    for (k, &y) in l.pys.iter().enumerate() {
        let g = blob(&l.px, y, cx, cy, sx, sy);
        let v = (y - l.ymin) / l.h.max(1e-9);
        let cv = (2.0 * PI * fy * v + phy).cos();
        let lr_true: Vec<f64> =
            l.px.iter()
                .zip(&g)
                .map(|(&x, &gi)| {
                    let u = (x - l.x0) / l.length.max(1e-9);
                    let bg = 0.08 * (2.0 * PI * fx * u + phx).sin() * cv;
                    lr_bg - depth * gi + bg
                })
                .collect();
        let n1 = rng.normals(0.0, e.noise, npk);
        out.push_line(k as i64 + 1, &l, y, relief(&l, y, f.z_base, f.z_amp));
        for i in 0..npk {
            out.rho_k.push(10f64.powf(lr_true[i] + n1[i]));
            out.rho_true.push(10f64.powf(lr_true[i]));
        }
        let n2 = rng.normals(0.0, 3.0, npk);
        out.sp
            .extend(g.iter().zip(&n2).map(|(gi, n)| e.sp_amp * gi + n));
        let n3 = rng.normals(0.0, 1.0, npk);
        out.vp
            .extend(g.iter().zip(&n3).map(|(gi, n)| e.vp_bg + e.vp_amp * gi + n));
    }
    Some(out)
}

/// Параметры оседаний (`gen_subsidence`).
#[derive(Debug, Clone, Copy)]
pub struct Subsidence {
    pub subs_max: f64,
    pub n_tours: i64,
    pub noise: f64,
    pub positive: bool,
}

/// Оседания: мульда по турам, по краям строго нули. `None` - пустой охват.
pub fn gen_subsidence(f: &Frame, s: &Subsidence, rng: &mut Rng) -> Option<Profiles> {
    let (xmin, ymin, xmax, ymax) = f.ext;
    let l = geometry(xmin, ymin, xmax, ymax, f.n_profiles, f.picket_step)?;
    let npk = l.px.len();
    let n_tours = s.n_tours.max(1);
    let subs_max = s.subs_max.abs().min(2000.0);
    let sgn = if s.positive { 1.0 } else { -1.0 };
    let cx = l.x0 + rng.uniform(0.4, 0.6) * l.length;
    let cy = (ymin + ymax) / 2.0 + rng.uniform(-0.1, 0.1) * l.h;
    let sx = 0.20 * l.length;
    let sy = 0.22 * l.h;
    let mut out = Profiles::default();
    for t in 1..=n_tours {
        let depth_t = subs_max * (t as f64 / n_tours as f64);
        for (k, &y) in l.pys.iter().enumerate() {
            let g: Vec<f64> = blob(&l.px, y, cx, cy, sx, sy)
                .into_iter()
                .map(|v| if v < 0.02 { 0.0 } else { v })
                .collect();
            let noise = rng.normals(0.0, s.noise, npk);
            out.push_line(k as i64 + 1, &l, y, relief(&l, y, f.z_base, f.z_amp));
            out.tour.extend(std::iter::repeat_n(t, npk));
            for i in 0..npk {
                let st = sgn * depth_t * g[i];
                out.settle_true.push(st);
                out.settle.push(st + noise[i] * g[i]);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_div_like_python() {
        assert_eq!(floor_div(900.0, 20.0), 45.0);
        assert_eq!(floor_div(7.0, 2.0), 3.0);
        assert_eq!(floor_div(-7.0, 2.0), -4.0);
        // 1 // 0.1 у Python - 9.0, хотя 1 / 0.1 = 10.0
        assert_eq!(floor_div(1.0, 0.1), 9.0);
    }

    #[test]
    fn pk_like_module() {
        assert_eq!(pk_parts(520.0), (5, 20));
        assert_eq!(pk_parts(0.0), (0, 0));
        assert_eq!(pk_parts(199.6), (2, 0));
        assert_eq!(pk_parts(12.5), (0, 12));
    }
}
