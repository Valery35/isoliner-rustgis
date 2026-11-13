// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Вариограммы, часть `grid_isolines/kb2d.py` модуля: экспериментальная
//! вариограмма (Матерон и Кресси-Хокинс, веса декластеризации), подбор
//! модели, проверки данных и пары, формирующие наггет. Порядок
//! сложений - как у NumPy.

use crate::npy_rng::Rng;
use crate::numpy;

pub const EPS: f64 = 1e-11;
pub const MODEL_SPHERICAL: usize = 0;
pub const MODEL_EXPONENTIAL: usize = 1;
pub const MODEL_GAUSSIAN: usize = 2;

/// Форма вариограммы γ/C одной структуры без наггета (`variogram_shape`).
pub fn variogram_shape(model: usize, h: &[f64], a: f64) -> Vec<f64> {
    let a = a.max(EPS);
    h.iter()
        .map(|&x| {
            let hr = x / a;
            match model {
                MODEL_SPHERICAL => {
                    if hr < 1.0 {
                        1.5 * hr - 0.5 * hr.powf(3.0)
                    } else {
                        1.0
                    }
                }
                MODEL_EXPONENTIAL => 1.0 - (-hr).exp(),
                _ => 1.0 - (-(hr * hr)).exp(),
            }
        })
        .collect()
}

/// Экспериментальная вариограмма.
#[derive(Debug, Clone, Default)]
pub struct Experimental {
    pub lag: Vec<f64>,
    pub gamma: Vec<f64>,
    pub npairs: Vec<usize>,
    pub maxlag: f64,
    pub width: f64,
    pub n_used: usize,
    pub subsampled: bool,
}

/// Омнинаправленная полувариограмма (`experimental_variogram`, без облака
/// пар). `maxlag <= 0` - половина диагонали охвата. При числе пар больше
/// `max_pairs` точки прореживаются выборкой генератора с зерном 0.
pub fn experimental_variogram(
    xs: &[f64],
    ys: &[f64],
    vs: &[f64],
    n_lags: i64,
    maxlag: f64,
    robust: bool,
    wts: Option<&[f64]>,
) -> Experimental {
    const MAX_PAIRS: usize = 6_000_000;
    let mut n = xs.len();
    let (mut xs, mut ys, mut vs) = (xs.to_vec(), ys.to_vec(), vs.to_vec());
    let mut w = wts.map(<[f64]>::to_vec);
    let mut subsampled = false;
    if n > 2 && n * (n - 1) / 2 > MAX_PAIRS {
        let m = ((1.0 + (1.0 + 8.0 * MAX_PAIRS as f64).sqrt()) / 2.0) as usize;
        let m = m.min(n).max(2);
        let mut rng = Rng::new(0);
        let idx = rng.choice(n, m).unwrap_or_default();
        let pick = |a: &[f64]| idx.iter().map(|&i| a[i]).collect::<Vec<f64>>();
        xs = pick(&xs);
        ys = pick(&ys);
        vs = pick(&vs);
        w = w.map(|w| pick(&w));
        n = m;
        subsampled = true;
    }
    let mut maxlag = maxlag;
    if maxlag.is_nan() || maxlag <= 0.0 {
        let span = |a: &[f64]| {
            a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - a.iter().copied().fold(f64::INFINITY, f64::min)
        };
        maxlag = 0.5 * span(&xs).hypot(span(&ys));
    }
    if maxlag <= 0.0 {
        maxlag = 1.0;
    }
    let nl = n_lags.max(1) as usize;
    let width = maxlag / nl as f64;
    let mut cnt = vec![0.0f64; nl];
    let mut sw = vec![0.0f64; nl];
    let mut s_h = vec![0.0; nl];
    let mut s_d2 = vec![0.0; nl];
    let mut s_sqrt = vec![0.0; nl];
    for i in 0..n.saturating_sub(1) {
        for j in i + 1..n {
            let hx = xs[j] - xs[i];
            let hy = ys[j] - ys[i];
            let h = (hx * hx + hy * hy).sqrt();
            if !(h > 0.0 && h <= maxlag) {
                continue;
            }
            let d = vs[j] - vs[i];
            let b = ((h / width) as usize).min(nl - 1);
            let wp = w.as_ref().map_or(1.0, |w| w[i] * w[j]);
            cnt[b] += 1.0;
            sw[b] += wp;
            s_h[b] += wp * h;
            s_d2[b] += wp * d * d;
            if robust {
                s_sqrt[b] += wp * d.abs().sqrt();
            }
        }
    }
    let mut out = Experimental {
        maxlag,
        width,
        n_used: n,
        subsampled,
        ..Experimental::default()
    };
    for k in 0..nl {
        if cnt[k] <= 0.0 {
            continue;
        }
        let safe = sw[k].max(1e-12);
        let cs = cnt[k].max(1.0);
        out.lag.push(s_h[k] / safe);
        out.gamma.push(if robust {
            let ms = s_sqrt[k] / safe;
            let denom = 0.457 + 0.494 / cs + 0.045 / (cs * cs);
            0.5 * ms.powf(4.0) / denom
        } else {
            0.5 * (s_d2[k] / safe)
        });
        out.npairs.push(cnt[k] as usize);
    }
    out
}

/// Взвешенный МНК для γ = c0 + C*s, c0 и C не меньше нуля (`_wls_c0_c`).
fn wls_c0_c(s: &[f64], y: &[f64], w: &[f64]) -> (f64, f64, f64) {
    let prod = |f: &dyn Fn(usize) -> f64| numpy::sum(&(0..s.len()).map(f).collect::<Vec<_>>());
    let sw = numpy::sum(w);
    let ss = prod(&|i| w[i] * s[i]);
    let sss = prod(&|i| w[i] * s[i] * s[i]);
    let sy = prod(&|i| w[i] * y[i]);
    let ssy = prod(&|i| w[i] * s[i] * y[i]);
    let det = sw * sss - ss * ss;
    let (mut c0, mut c) = (0.0, 0.0);
    if det.abs() > 1e-30 {
        c0 = (sy * sss - ss * ssy) / det;
        c = (sw * ssy - ss * sy) / det;
    }
    let sse = |a0: f64, a1: f64| {
        prod(&|i| {
            let r = y[i] - a0 - a1 * s[i];
            w[i] * r * r
        })
    };
    if c0 < 0.0 || c < 0.0 || det.abs() <= 1e-30 {
        let mut cands = vec![
            (0.0, (if sss > 1e-30 { ssy / sss } else { 0.0 }).max(0.0)),
            (if sw > 0.0 { sy / sw } else { 0.0 }, 0.0),
        ];
        if c0 >= 0.0 && c >= 0.0 {
            cands.push((c0, c));
        }
        let mut best: Option<(f64, f64, f64)> = None;
        for (a0, a1) in cands {
            let e = sse(a0, a1);
            if best.is_none_or(|b| e < b.2) {
                best = Some((a0, a1, e));
            }
        }
        return best.unwrap_or((0.0, 0.0, f64::INFINITY));
    }
    (c0, c, sse(c0, c))
}

/// `np.geomspace(lo, hi, n)` для положительных концов.
fn geomspace(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    let mut out: Vec<f64> = numpy::linspace(lo.log10(), hi.log10(), n)
        .into_iter()
        .map(|y| 10f64.powf(y))
        .collect();
    if let Some(f) = out.first_mut() {
        *f = lo;
    }
    if n > 1
        && let Some(l) = out.last_mut()
    {
        *l = hi;
    }
    out
}

/// Подобранная модель.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub model: usize,
    pub nugget: f64,
    pub sill: f64,
    pub range: f64,
    pub r2: f64,
    pub npts: usize,
}

/// Подбор модели вариограммы (`fit_variogram`): a перебором по
/// геометрической сетке с уточнением, (c0, C) взвешенным МНК, плато
/// прижато к 1.15 max γ. `model` - `None` для выбора лучшей по R².
pub fn fit_variogram(
    lag: &[f64],
    gamma: &[f64],
    npairs: &[usize],
    model: Option<usize>,
) -> Option<Fit> {
    let mut l = Vec::new();
    let mut g = Vec::new();
    let mut w = Vec::new();
    for i in 0..lag.len() {
        if lag[i].is_finite() && gamma[i].is_finite() && lag[i] > 0.0 {
            l.push(lag[i]);
            g.push(gamma[i]);
            w.push((npairs[i] as f64).max(1.0));
        }
    }
    if l.len() < 3 {
        return None;
    }
    let gmax = g.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let sill_cap = 1.15 * gmax;
    let lmax = l.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lmin = l.iter().copied().fold(f64::INFINITY, f64::min);
    let a_lo = (0.05 * lmax).max(lmin * 0.5).max(EPS);
    let a_hi = 1.5 * lmax;
    let grid = geomspace(a_lo, a_hi, 80);
    let models: Vec<usize> = match model {
        None => vec![MODEL_SPHERICAL, MODEL_EXPONENTIAL, MODEL_GAUSSIAN],
        Some(m) => vec![m],
    };
    let sw = numpy::sum(&w);
    let wy: Vec<f64> = w.iter().zip(&g).map(|(a, b)| a * b).collect();
    let ybar = numpy::sum(&wy) / sw;
    let dev: Vec<f64> = w
        .iter()
        .zip(&g)
        .map(|(a, b)| {
            let d = b - ybar;
            a * (d * d)
        })
        .collect();
    let sst = {
        let s = numpy::sum(&dev);
        if s == 0.0 { 1.0 } else { s }
    };
    let eval = |m: usize, a: f64| {
        let s = variogram_shape(m, &l, a);
        let (mut c0, mut c, mut sse) = wls_c0_c(&s, &g, &w);
        let total = c0 + c;
        if sill_cap != 0.0 && total > sill_cap && total > 0.0 {
            let f = sill_cap / total;
            c0 *= f;
            c *= f;
            let e: Vec<f64> = (0..l.len())
                .map(|i| {
                    let r = g[i] - c0 - c * s[i];
                    w[i] * r * r
                })
                .collect();
            sse = numpy::sum(&e);
        }
        (c0, c, sse)
    };
    let mut best: Option<Fit> = None;
    for m in models {
        let coarse: Vec<f64> = grid.iter().map(|&a| eval(m, a).2).collect();
        let mut j = 0;
        for (k, e) in coarse.iter().enumerate() {
            if *e < coarse[j] {
                j = k;
            }
        }
        let mut alo = grid[j.saturating_sub(1)];
        let mut ahi = grid[(j + 1).min(grid.len() - 1)];
        for _ in 0..40 {
            let a1 = alo * (ahi / alo).powf(1.0 / 3.0);
            let a2 = alo * (ahi / alo).powf(2.0 / 3.0);
            if eval(m, a1).2 < eval(m, a2).2 {
                ahi = a2;
            } else {
                alo = a1;
            }
            if ahi / alo < 1.0001 {
                break;
            }
        }
        let a_ref = (alo * ahi).sqrt();
        let (c0, c, sse) = eval(m, a_ref);
        let cand = Fit {
            model: m,
            nugget: c0.max(0.0),
            sill: c.max(0.0),
            range: a_ref,
            r2: 1.0 - sse / sst,
            npts: l.len(),
        };
        if best.is_none_or(|b| cand.r2 > b.r2) {
            best = Some(cand);
        }
    }
    best
}

/// Проверки данных (`data_warnings`): мало точек, совпадающие
/// координаты, постоянные значения.
pub enum DataWarning {
    FewPoints(usize),
    Duplicates(usize),
    Constant,
}

pub fn data_warnings(xs: &[f64], ys: &[f64], vs: &[f64]) -> Vec<DataWarning> {
    let n = vs.len();
    let mut out = Vec::new();
    if n < 8 {
        out.push(DataWarning::FewPoints(n));
    }
    if n > 1 {
        // множество пар координат, -0.0 и 0.0 - одна точка, как в Python
        let key = |v: f64| {
            if v == 0.0 {
                0.0f64.to_bits()
            } else {
                v.to_bits()
            }
        };
        let mut set: Vec<(u64, u64)> = xs.iter().zip(ys).map(|(x, y)| (key(*x), key(*y))).collect();
        set.sort_unstable();
        set.dedup();
        let dup = n - set.len();
        if dup > 0 {
            out.push(DataWarning::Duplicates(dup));
        }
    }
    if n > 0 {
        let lo = vs.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = vs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if hi - lo == 0.0 {
            out.push(DataWarning::Constant);
        }
    }
    out
}

/// Пара, формирующая наггет: номера, расстояние, значения, вклад γ.
pub type NuggetPair = (usize, usize, f64, f64, f64, f64);

/// Пары ближе `maxdist` с наибольшим вкладом 0.5 (vi - vj)² (`nugget_pairs`).
pub fn nugget_pairs(
    xs: &[f64],
    ys: &[f64],
    vs: &[f64],
    maxdist: f64,
    top: usize,
) -> Vec<NuggetPair> {
    let n = vs.len();
    if n < 2 || maxdist.is_nan() || maxdist <= 0.0 {
        return Vec::new();
    }
    let md2 = maxdist * maxdist;
    let mut best: Vec<NuggetPair> = Vec::new();
    for i in 0..n - 1 {
        for j in i + 1..n {
            let dx = xs[i] - xs[j];
            let dy = ys[i] - ys[j];
            let d2 = dx * dx + dy * dy;
            if d2 <= md2 {
                let dv = vs[i] - vs[j];
                best.push((i, j, d2.sqrt(), vs[i], vs[j], 0.5 * (dv * dv)));
            }
        }
    }
    // устойчивая сортировка по убыванию вклада, как sort(key=-g)
    best.sort_by(|a, b| b.5.total_cmp(&a.5));
    best.truncate(top);
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_reaches_sill() {
        let s = variogram_shape(MODEL_SPHERICAL, &[0.0, 50.0, 100.0, 200.0], 100.0);
        assert_eq!(s[0], 0.0);
        assert_eq!(s[2], 1.0);
        assert_eq!(s[3], 1.0);
        assert!((s[1] - 0.6875).abs() < 1e-12);
    }

    #[test]
    fn fit_recovers_spherical() {
        let lag: Vec<f64> = (1..=15).map(|k| f64::from(k) * 20.0).collect();
        let gamma: Vec<f64> = variogram_shape(MODEL_SPHERICAL, &lag, 180.0)
            .iter()
            .map(|s| 2.0 + 10.0 * s)
            .collect();
        let f = fit_variogram(&lag, &gamma, &[100; 15], Some(MODEL_SPHERICAL)).unwrap_or(Fit {
            model: 9,
            nugget: 0.0,
            sill: 0.0,
            range: 0.0,
            r2: 0.0,
            npts: 0,
        });
        assert!((f.range - 180.0).abs() < 1.0, "{f:?}");
        assert!(
            (f.nugget - 2.0).abs() < 0.05 && (f.sill - 10.0).abs() < 0.05,
            "{f:?}"
        );
        assert!(f.r2 > 0.999);
    }
}
