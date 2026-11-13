// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Точечный кригинг, порт ядра `grid_isolines/kb2d.py` (GSLIB KB2D):
//! вариограмма с вложенными структурами и анизотропией (`Variogram`,
//! `cova2_array`), кригинг в точке с тем же отбором соседей
//! (`_solve_point`), скользящий контроль (`cross_validate`,
//! `cross_validate_detrend`) и полиномиальный тренд (`PolyTrend`).
//!
//! Система решается разложением LU с выбором главного элемента, как
//! `np.linalg.solve`. Порядок операций LAPACK не повторяется, поэтому
//! последние разряды могут отличаться от модуля, сверка идёт с допуском.

use crate::numpy;

pub const EPS: f64 = 1e-11;
/// Доля структурного силла гауссовой модели, ниже которой поднимается наггет.
pub const GAUSS_MIN_NUGGET_FRAC: f64 = 0.01;
const DTOR: f64 = std::f64::consts::PI / 180.0;

/// Структура вариограммы: модель GSLIB (1 сферическая, 2 экспоненциальная,
/// 3 гауссова, 4 степенная), вклад, радиус, азимут, анизотропия.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Structure {
    pub it: i64,
    pub cc: f64,
    pub aa: f64,
    pub ang: f64,
    pub anis: f64,
}

/// Наггет и вложенные структуры (`Variogram`).
#[derive(Debug, Clone)]
pub struct Variogram {
    pub c0: f64,
    pub it: Vec<i64>,
    pub cc: Vec<f64>,
    pub aa: Vec<f64>,
    pub anis: Vec<f64>,
    rot: Vec<[f64; 4]>,
    pub maxcov: f64,
    /// Степенные структуры с показателем вне (0; 2): номер с единицы и
    /// исходное значение.
    pub power_clamped: Vec<(usize, f64)>,
    /// Наггет гауссовой модели до подъёма.
    pub nugget_raised_from: Option<f64>,
}

const PMX: f64 = 9999.0;

impl Variogram {
    pub fn new(nugget: f64, s: &[Structure]) -> Self {
        let mut c0 = nugget;
        let it: Vec<i64> = s.iter().map(|x| x.it).collect();
        let cc: Vec<f64> = s.iter().map(|x| x.cc).collect();
        let mut aa: Vec<f64> = s.iter().map(|x| x.aa.max(EPS)).collect();
        let mut power_clamped = Vec::new();
        for (i, x) in s.iter().enumerate() {
            if x.it == 4 && !(0.05..2.0).contains(&x.aa) {
                power_clamped.push((i + 1, x.aa));
                aa[i] = x.aa.clamp(0.05, 1.999);
            }
        }
        let anis: Vec<f64> = s.iter().map(|x| x.anis.max(EPS)).collect();
        let mut nugget_raised_from = None;
        let gauss = numpy::py_sum(
            it.iter()
                .zip(&cc)
                .filter(|(t, _)| **t == 3)
                .map(|(_, c)| *c),
        );
        if gauss > 0.0 {
            let min_c0 = GAUSS_MIN_NUGGET_FRAC * gauss;
            if c0 < min_c0 {
                nugget_raised_from = Some(c0);
                c0 = min_c0;
            }
        }
        let mut maxcov = c0;
        let mut rot = Vec::new();
        for (i, x) in s.iter().enumerate() {
            let az = (90.0 - x.ang) * DTOR;
            let (ca, sa) = (az.cos(), az.sin());
            rot.push([ca, sa, -sa, ca]);
            maxcov += if it[i] == 4 { PMX } else { cc[i] };
        }
        Variogram {
            c0,
            it,
            cc,
            aa,
            anis,
            rot,
            maxcov,
            power_clamped,
            nugget_raised_from,
        }
    }

    /// Ковариация для вектора разности (`cova2_array` поэлементно).
    pub fn cova(&self, dx: f64, dy: f64) -> f64 {
        if dx * dx + dy * dy < EPS {
            return self.maxcov;
        }
        let mut cov = 0.0;
        for i in 0..self.it.len() {
            let [r0, r1, r2, r3] = self.rot[i];
            let dx1 = dx * r0 + dy * r1;
            let dy1 = (dx * r2 + dy * r3) / self.anis[i];
            let h = (dx1 * dx1 + dy1 * dy1).max(0.0).sqrt();
            let (cc, aa) = (self.cc[i], self.aa[i]);
            cov += match self.it[i] {
                1 => {
                    let hr = h / aa;
                    if hr < 1.0 {
                        cc * (1.0 - hr * (1.5 - 0.5 * hr * hr))
                    } else {
                        0.0
                    }
                }
                2 => cc * (-h / aa).exp(),
                3 => cc * (-(h * h) / (aa * aa)).exp(),
                // h ** aa массива NumPy: показатели 0.5 и 1 идут особым путём
                _ => {
                    let p = if aa == 1.0 {
                        h
                    } else if aa == 0.5 {
                        h.sqrt()
                    } else {
                        h.powf(aa)
                    };
                    PMX - cc * p
                }
            };
        }
        cov
    }
}

/// Решение A s = r разложением LU с выбором главного элемента по
/// столбцу. `None` - вырожденная матрица (нулевой главный элемент).
// индексы матрицы читаются яснее итераторов
#[allow(clippy::needless_range_loop)]
pub fn solve(mut a: Vec<Vec<f64>>, mut r: Vec<f64>) -> Option<Vec<f64>> {
    let n = r.len();
    for k in 0..n {
        let mut p = k;
        for i in k + 1..n {
            if a[i][k].abs() > a[p][k].abs() {
                p = i;
            }
        }
        if a[p][k] == 0.0 {
            return None;
        }
        a.swap(k, p);
        r.swap(k, p);
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            if f != 0.0 {
                for j in k + 1..n {
                    a[i][j] -= f * a[k][j];
                }
                r[i] -= f * r[k];
            }
        }
    }
    let mut s = vec![0.0; n];
    for k in (0..n).rev() {
        let mut v = r[k];
        for j in k + 1..n {
            v -= a[k][j] * s[j];
        }
        s[k] = v / a[k][k];
    }
    Some(s)
}

/// Ключ ничьей и порядка уравнений: X, затем Y, затем значение, затем номер.
fn data_order(xd: &[f64], yd: &[f64], vd: &[f64], a: usize, b: usize) -> std::cmp::Ordering {
    xd[a]
        .total_cmp(&xd[b])
        .then(yd[a].total_cmp(&yd[b]))
        .then(vd[a].total_cmp(&vd[b]))
        .then(a.cmp(&b))
}

/// Кригинг в точке (`_solve_point`, точечный). `ktype` 0 - простой,
/// 1 - ординарный. Возвращает (оценка, дисперсия), `nodata` при нехватке
/// соседей.
#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
pub fn solve_point(
    xloc: f64,
    yloc: f64,
    xd: &[f64],
    yd: &[f64],
    vrd: &[f64],
    vg: &Variogram,
    ktype: usize,
    skmean: f64,
    ndmin: usize,
    ndmax: usize,
    rad2: f64,
    nodata: f64,
) -> (f64, f64) {
    let n = xd.len();
    let h2: Vec<f64> = (0..n)
        .map(|i| {
            let dx = xd[i] - xloc;
            let dy = yd[i] - yloc;
            dx * dx + dy * dy
        })
        .collect();
    // ndmax ближайших; ничья на границе отбора решается по данным
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| h2[a].total_cmp(&h2[b]).then(data_order(xd, yd, vrd, a, b)));
    let mut sel: Vec<usize> = idx
        .into_iter()
        .take(ndmax)
        .filter(|&i| h2[i] <= rad2)
        .collect();
    sel.sort_by(|&a, &b| data_order(xd, yd, vrd, a, b));
    let na = sel.len();
    if na < ndmin {
        return (nodata, nodata);
    }
    let xa: Vec<f64> = sel.iter().map(|&i| xd[i]).collect();
    let ya: Vec<f64> = sel.iter().map(|&i| yd[i]).collect();
    let vra: Vec<f64> = sel.iter().map(|&i| vrd[i]).collect();
    let h2sel: Vec<f64> = sel.iter().map(|&i| h2[i]).collect();
    let c0pt = vg.maxcov;
    let cbb = c0pt;
    let imin = numpy::arg_extreme(&h2sel, false);
    if h2sel[imin] < EPS {
        return (vra[imin], 0.0);
    }
    let rhs: Vec<f64> = (0..na)
        .map(|i| vg.cova(xa[i] - xloc, ya[i] - yloc))
        .collect();
    if na == 1 {
        let cb = rhs[0];
        if ktype == 0 {
            let s = cb / c0pt;
            return (s * vra[0] + (1.0 - s) * skmean, (cbb - s * cb).max(0.0));
        }
        return (vra[0], (cbb - 2.0 * cb + c0pt).max(0.0));
    }
    let vmin = vra.iter().copied().fold(f64::INFINITY, f64::min);
    let vmax = vra.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = {
        let s = vmax - vmin;
        if s == 0.0 { vmax.abs() + 1.0 } else { s }
    };
    let (lo, hi) = (vmin - 3.0 * span, vmax + 3.0 * span);
    let jitter = c0pt * 1e-9;
    let neq = na + ktype;
    let mut a = vec![vec![0.0; neq]; neq];
    for i in 0..na {
        for j in 0..na {
            a[i][j] = if i == j {
                c0pt + jitter
            } else {
                vg.cova(xa[i] - xa[j], ya[i] - ya[j])
            };
        }
    }
    let mut r = rhs.clone();
    if ktype == 1 {
        for i in 0..na {
            a[na][i] = vg.maxcov;
            a[i][na] = vg.maxcov;
        }
        a[na][na] = 0.0;
        r.push(vg.maxcov);
    }
    let mut est = None;
    let mut var = None;
    if let Some(s) = solve(a, r.clone())
        && s.iter().all(|x| x.is_finite())
    {
        let w = &s[..na];
        let mut e: f64 = w.iter().zip(&vra).map(|(a, b)| a * b).sum();
        if ktype == 0 {
            e += (1.0 - numpy::sum(w)) * skmean;
        }
        let mut v = cbb - w.iter().zip(&r[..na]).map(|(a, b)| a * b).sum::<f64>();
        if ktype == 1 {
            v -= vg.maxcov * s[na];
        }
        if e.is_finite() && lo <= e && e <= hi {
            est = Some(e);
            var = Some(v.max(0.0));
        }
    }
    match (est, var) {
        (Some(e), Some(v)) => (e, v),
        _ => {
            // запас: обратные расстояния до тех же соседей
            let wts: Vec<f64> = h2sel.iter().map(|h| 1.0 / h.max(EPS)).collect();
            let num: f64 = wts.iter().zip(&vra).map(|(a, b)| a * b).sum();
            (num / numpy::sum(&wts), cbb)
        }
    }
}

/// Параметры поиска и типа кригинга.
#[derive(Debug, Clone, Copy)]
pub struct Search {
    pub ktype: usize,
    pub skmean: f64,
    pub ndmin: usize,
    pub ndmax: usize,
    pub rad2: f64,
    pub nodata: f64,
}

fn without<T: Copy>(a: &[T], i: usize) -> Vec<T> {
    a.iter()
        .enumerate()
        .filter(|(k, _)| *k != i)
        .map(|(_, v)| *v)
        .collect()
}

/// Скользящий контроль (`cross_validate`): оценка каждой точки по остальным.
pub fn cross_validate(
    xd: &[f64],
    yd: &[f64],
    vrd: &[f64],
    vg: &Variogram,
    s: &Search,
) -> (Vec<f64>, Vec<f64>) {
    let n = xd.len();
    let mut est = vec![s.nodata; n];
    let mut var = vec![s.nodata; n];
    for i in 0..n {
        let (e, v) = solve_point(
            xd[i],
            yd[i],
            &without(xd, i),
            &without(yd, i),
            &without(vrd, i),
            vg,
            s.ktype,
            s.skmean,
            s.ndmin,
            s.ndmax,
            s.rad2,
            s.nodata,
        );
        est[i] = e;
        var[i] = v;
    }
    (est, var)
}

/// Скользящий контроль с трендом (`cross_validate_detrend`): тренд
/// переподбирается по остальным точкам, кригуются остатки.
pub fn cross_validate_detrend(
    xd: &[f64],
    yd: &[f64],
    vrd: &[f64],
    degree: usize,
    vg: &Variogram,
    s: &Search,
) -> (Vec<f64>, Vec<f64>) {
    let n = xd.len();
    let mut est = vec![s.nodata; n];
    let mut var = vec![s.nodata; n];
    for i in 0..n {
        let (xi, yi, zi) = (without(xd, i), without(yd, i), without(vrd, i));
        let tr = PolyTrend::fit(&xi, &yi, &zi, degree);
        let ri: Vec<f64> = (0..xi.len()).map(|k| zi[k] - tr.at(xi[k], yi[k])).collect();
        let (e, v) = solve_point(
            xd[i], yd[i], &xi, &yi, &ri, vg, s.ktype, s.skmean, s.ndmin, s.ndmax, s.rad2, s.nodata,
        );
        if e != s.nodata {
            est[i] = tr.at(xd[i], yd[i]) + e;
            var[i] = v;
        }
    }
    (est, var)
}

/// Полиномиальный тренд степени 1 или 2 по центрированным и
/// масштабированным координатам (`PolyTrend`).
#[derive(Debug, Clone)]
pub struct PolyTrend {
    beta: Vec<f64>,
    x0: f64,
    y0: f64,
    sx: f64,
    sy: f64,
    degree: usize,
}

impl PolyTrend {
    pub fn n_terms(degree: usize) -> usize {
        if degree == 1 { 3 } else { 6 }
    }

    fn row(&self, x: f64, y: f64) -> Vec<f64> {
        let xx = (x - self.x0) / self.sx;
        let yy = (y - self.y0) / self.sy;
        let mut r = vec![1.0, xx, yy];
        if self.degree >= 2 {
            r.extend([xx * xx, xx * yy, yy * yy]);
        }
        r
    }

    /// Подбор МНК (Хаусхолдер QR вместо SVD `lstsq`).
    pub fn fit(x: &[f64], y: &[f64], z: &[f64], degree: usize) -> Self {
        let degree = if degree >= 2 { 2 } else { 1 };
        let nz = |v: f64| if v == 0.0 { 1.0 } else { v };
        let mut t = PolyTrend {
            beta: vec![0.0; Self::n_terms(degree)],
            x0: numpy::mean(x),
            y0: numpy::mean(y),
            sx: nz(numpy::var(x).sqrt()),
            sy: nz(numpy::var(y).sqrt()),
            degree,
        };
        let b: Vec<Vec<f64>> = (0..x.len()).map(|i| t.row(x[i], y[i])).collect();
        t.beta = lstsq(b, z.to_vec());
        t
    }

    pub fn at(&self, x: f64, y: f64) -> f64 {
        self.row(x, y)
            .iter()
            .zip(&self.beta)
            .map(|(a, b)| a * b)
            .sum()
    }
}

/// МНК: min |A b - z| отражениями Хаусхолдера. Для вырожденного столбца
/// коэффициент - ноль.
#[allow(clippy::needless_range_loop)]
fn lstsq(mut a: Vec<Vec<f64>>, mut z: Vec<f64>) -> Vec<f64> {
    let m = a.len();
    let n = a.first().map_or(0, Vec::len);
    let mut diag = vec![0.0; n];
    for k in 0..n.min(m) {
        let norm = (k..m).map(|i| a[i][k] * a[i][k]).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        let alpha = if a[k][k] > 0.0 { -norm } else { norm };
        let mut v: Vec<f64> = (k..m).map(|i| a[i][k]).collect();
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|x| x * x).sum();
        if vv == 0.0 {
            diag[k] = alpha;
            continue;
        }
        for j in k..n {
            let d: f64 = (k..m).map(|i| v[i - k] * a[i][j]).sum::<f64>() * 2.0 / vv;
            for i in k..m {
                a[i][j] -= d * v[i - k];
            }
        }
        let d: f64 = (k..m).map(|i| v[i - k] * z[i]).sum::<f64>() * 2.0 / vv;
        for i in k..m {
            z[i] -= d * v[i - k];
        }
        diag[k] = a[k][k];
    }
    let mut b = vec![0.0; n];
    for k in (0..n.min(m)).rev() {
        if diag[k] == 0.0 {
            continue;
        }
        let mut s = z[k];
        for j in k + 1..n {
            s -= a[k][j] * b[j];
        }
        b[k] = s / a[k][k];
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sph(c0: f64, cc: f64, aa: f64) -> Variogram {
        Variogram::new(
            c0,
            &[Structure {
                it: 1,
                cc,
                aa,
                ang: 0.0,
                anis: 1.0,
            }],
        )
    }

    #[test]
    fn exact_at_sample_and_unbiased() {
        let xd = [0.0, 10.0, 0.0, 10.0];
        let yd = [0.0, 0.0, 10.0, 10.0];
        let vd = [1.0, 2.0, 3.0, 4.0];
        let vg = sph(0.0, 1.0, 30.0);
        let (e, v) = solve_point(0.0, 0.0, &xd, &yd, &vd, &vg, 1, 0.0, 1, 8, 1e12, -9999.0);
        assert_eq!((e, v), (1.0, 0.0));
        // центр квадрата: симметрия, среднее
        let (e, _) = solve_point(5.0, 5.0, &xd, &yd, &vd, &vg, 1, 0.0, 1, 8, 1e12, -9999.0);
        assert!((e - 2.5).abs() < 1e-9, "{e}");
    }

    #[test]
    fn plane_trend_is_exact() {
        let x: Vec<f64> = (0..20).map(|i| f64::from(i % 5) * 10.0).collect();
        let y: Vec<f64> = (0..20).map(|i| f64::from(i / 5) * 7.0).collect();
        let z: Vec<f64> = x
            .iter()
            .zip(&y)
            .map(|(a, b)| 3.0 + 0.5 * a - 0.2 * b)
            .collect();
        let t = PolyTrend::fit(&x, &y, &z, 1);
        assert!((t.at(13.0, 4.0) - (3.0 + 6.5 - 0.8)).abs() < 1e-9);
    }

    #[test]
    fn solve_small_system() {
        let s = solve(vec![vec![2.0, 1.0], vec![1.0, 3.0]], vec![3.0, 5.0]).unwrap_or_default();
        assert!((s[0] - 0.8).abs() < 1e-12 && (s[1] - 1.4).abs() < 1e-12);
        assert!(solve(vec![vec![1.0, 2.0], vec![2.0, 4.0]], vec![1.0, 2.0]).is_none());
    }
}
