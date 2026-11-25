// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Мультисеточные B-сплайны (MBA): поверхность по разбросанным точкам.
//! Порт `grid_isolines/mba.py` модуля QGIS. Метод Ли, Волберга и Шина
//! (1997): грубая решётка → кубический B-сплайн → остаток → решётка
//! удваивается → следующий уровень. Система уравнений не решается:
//! коэффициент - взвешенная сумма по точкам в носителе сплайна.
//! Работает линейно по числу точек.

/// Носитель кубического B-сплайна: четыре ячейки по каждой оси.
pub const SUPPORT: usize = 4;

/// Кубические B-сплайны в точке t внутри ячейки, t в [0, 1).
/// Возвращает (4, n): вклад четырёх соседних контрольных точек.
/// Сумма по четырём равна единице при любом t — разбиение единицы.
fn weights_1d(t: &[f64]) -> Vec<[f64; 4]> {
    t.iter()
        .map(|&ti| {
            let omt = 1.0 - ti;
            [
                omt * omt * omt / 6.0,
                (3.0 * ti * ti * ti - 6.0 * ti * ti + 4.0) / 6.0,
                (-3.0 * ti * ti * ti + 3.0 * ti * ti + 3.0 * ti + 1.0) / 6.0,
                ti * ti * ti / 6.0,
            ]
        })
        .collect()
}

/// Смещения носителя по осям: 2^ndim комбинаций.
fn offsets(ndim: usize) -> Vec<Vec<usize>> {
    let n = 1usize << ndim;
    (0..n)
        .map(|k| (0..ndim).map(|d| (k >> d) & 1).collect())
        .collect()
}

/// Решётка контрольных точек одного уровня.
#[derive(Debug, Clone)]
pub struct Lattice {
    /// Значения контрольных точек.
    pub data: Vec<f64>,
    /// Число контрольных точек по каждой оси.
    pub shape: Vec<usize>,
    /// Нижний угол области.
    pub lo: Vec<f64>,
    /// Шаг по каждой оси.
    pub step: Vec<f64>,
}

impl Lattice {
    /// Новая решётка с нулями.
    pub fn zeros(shape: &[usize], lo: &[f64], step: &[f64]) -> Self {
        let total: usize = shape.iter().product();
        Self {
            data: vec![0.0; total],
            shape: shape.to_vec(),
            lo: lo.to_vec(),
            step: step.to_vec(),
        }
    }

    /// Линейный индекс по координатам решётки.
    fn idx(&self, c: &[usize]) -> usize {
        let mut i = c[0];
        for d in 1..c.len() {
            i = i * self.shape[d] + c[d];
        }
        i
    }

    /// Прибавление к значению.
    fn add(&mut self, c: &[usize], v: f64) {
        let i = self.idx(c);
        self.data[i] += v;
    }

    /// Значение сплайна в одной точке p (ndim координат).
    pub fn at(&self, p: &[f64]) -> f64 {
        let ndim = self.shape.len();
        let mut cell = vec![0usize; ndim];
        let mut frac = vec![0.0f64; ndim];
        for d in 0..ndim {
            let u = (p[d] - self.lo[d]) / self.step[d];
            let fl = u.floor();
            cell[d] = (fl as usize).saturating_sub(1);
            frac[d] = u - fl;
        }
        let w = weights_1d(&frac);
        let mut val = 0.0;
        for off in offsets(ndim).iter() {
            let mut coeff = 1.0;
            let mut c = vec![0usize; ndim];
            let mut inside = true;
            for d in 0..ndim {
                c[d] = cell[d] + off[d];
                if c[d] >= self.shape[d] {
                    inside = false;
                    break;
                }
                coeff *= w[d][off[d]];
            }
            if inside && coeff != 0.0 {
                val += coeff * self.data[self.idx(&c)];
            }
        }
        val
    }
}

/// Тренд: константа или плоскость, вычитается перед уровнями.
fn trend(pts: &[f64], vals: &[f64], n: usize, ndim: usize, mode: &str) -> (Vec<f64>, f64) {
    match mode {
        "const" => {
            let mean = crate::numpy::sum(vals) / n as f64;
            (vec![0.0; ndim], mean)
        }
        _ => {
            // Плоскость по МНК: z = a·x + b·y + ... + c.
            let dim = ndim + 1;
            let mut ata = vec![vec![0.0; dim]; dim];
            let mut atb = vec![0.0; dim];
            for k in 0..n {
                let mut row = vec![1.0; dim];
                for d in 0..ndim {
                    row[d] = pts[k * ndim + d];
                }
                for (i, &ri) in row.iter().enumerate() {
                    atb[i] += ri * vals[k];
                    for (j, &rj) in row.iter().enumerate() {
                        ata[i][j] += ri * rj;
                    }
                }
            }
            // Гаусс.
            for col in 0..dim {
                let piv = ata[col][col];
                if piv.abs() < 1e-30 {
                    continue;
                }
                for j in (col + 1)..dim {
                    let f = ata[j][col] / piv;
                    for i in col..dim {
                        ata[j][i] -= f * ata[col][i];
                    }
                    atb[j] -= f * atb[col];
                }
            }
            let mut sol = vec![0.0; dim];
            for i in (0..dim).rev() {
                let mut s = atb[i];
                for j in (i + 1)..dim {
                    s -= ata[i][j] * sol[j];
                }
                if ata[i][i].abs() > 1e-30 {
                    sol[i] = s / ata[i][i];
                }
            }
            (sol[..ndim].to_vec(), sol[ndim])
        }
    }
}

/// Результат подгонки: решётки уровней и тренд.
pub struct Fit {
    /// Решётки от грубой к мелкой.
    pub lattices: Vec<Lattice>,
    /// Коэффициенты плоскости тренда (a·x + b·y + ...).
    pub trend_a: Vec<f64>,
    /// Свободный член тренда.
    pub trend_c: f64,
    /// RMS остатка последнего уровня.
    pub rms: f64,
}

/// Подгонка поверхности.
/// pts — координаты (n × ndim, подряд), vals — значения (n),
/// grid — стартовые ячейки по осям, levels — уровней уточнения.
pub fn fit(
    pts: &[f64],
    vals: &[f64],
    lo: Option<&[f64]>,
    hi: Option<&[f64]>,
    grid: &[usize],
    levels: usize,
    tol: Option<f64>,
    trend_mode: &str,
) -> Fit {
    let n = vals.len();
    let ndim = grid.len();

    // Область.
    let lo_v: Vec<f64> = lo.map(|l| l.to_vec()).unwrap_or_else(|| {
        (0..ndim)
            .map(|d| (0..n).map(|k| pts[k * ndim + d]).fold(f64::INFINITY, f64::min))
            .collect()
    });
    let hi_v: Vec<f64> = hi.map(|h| h.to_vec()).unwrap_or_else(|| {
        (0..ndim)
            .map(|d| (0..n).map(|k| pts[k * ndim + d]).fold(f64::NEG_INFINITY, f64::max))
            .collect()
    });

    // Тренд и остатки.
    let (ta, tc) = trend(pts, vals, n, ndim, trend_mode);
    let mut resid: Vec<f64> = (0..n)
        .map(|k| {
            let t: f64 = ta.iter().zip(0..ndim).map(|(&a, d)| a * pts[k * ndim + d]).sum();
            vals[k] - t - tc
        })
        .collect();

    // Стартовая решётка.
    let mut cur_shape: Vec<usize> = grid.to_vec();
    let mut cur_step: Vec<f64> = (0..ndim)
        .map(|d| {
            let s = hi_v[d] - lo_v[d];
            if s == 0.0 { 1.0 } else { s / cur_shape[d] as f64 }
        })
        .collect();

    let mut lattices = Vec::with_capacity(levels);
    let mut rms = f64::INFINITY;

    for level in 0..levels {
        let mut lat = Lattice::zeros(&cur_shape, &lo_v, &cur_step);
        // Вклад точек в контрольные.
        for k in 0..n {
            let mut cell = vec![0usize; ndim];
            let mut frac = vec![0.0f64; ndim];
            for d in 0..ndim {
                let u = (pts[k * ndim + d] - lo_v[d]) / cur_step[d];
                let fl = u.floor();
                cell[d] = (fl as usize).saturating_sub(1);
                frac[d] = u - fl;
            }
            let w = weights_1d(&frac);
            for off in offsets(ndim).iter() {
                let mut cw = 1.0;
                let mut c = vec![0usize; ndim];
                let mut inside = true;
                for d in 0..ndim {
                    c[d] = cell[d] + off[d];
                    if c[d] >= cur_shape[d] {
                        inside = false;
                        break;
                    }
                    cw *= w[d][off[d]];
                }
                if inside && cw != 0.0 {
                    lat.add(&c, cw * resid[k]);
                }
            }
        }
        // Остатки после уровня.
        for k in 0..n {
            let p: Vec<f64> = (0..ndim).map(|d| pts[k * ndim + d]).collect();
            resid[k] -= lat.at(&p);
        }
        lattices.push(lat);
        // RMS.
        rms = (resid.iter().map(|r| r * r).sum::<f64>() / n as f64).sqrt();
        // Досрочный выход.
        if let Some(t) = tol {
            if rms < t {
                break;
            }
        }
        // Решётка удваивается.
        if level + 1 < levels {
            for d in 0..ndim {
                cur_shape[d] = (cur_shape[d] - 1) * 2 + 1;
                cur_step[d] /= 2.0;
            }
        }
    }

    Fit { lattices, trend_a: ta, trend_c: tc, rms }
}

/// Оценка поверхности в точках (n × ndim).
pub fn evaluate(f: &Fit, pts: &[f64], ndim: usize) -> Vec<f64> {
    let n = pts.len() / ndim;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut v = f.trend_c;
        for d in 0..ndim {
            v += f.trend_a[d] * pts[k * ndim + d];
        }
        for lat in &f.lattices {
            let p: Vec<f64> = (0..ndim).map(|d| pts[k * ndim + d]).collect();
            v += lat.at(&p);
        }
        out.push(v);
    }
    out
}

/// Оценка на регулярной сетке. Отдаёт (значения, shape).
pub fn evaluate_grid(f: &Fit, axes: &[Vec<f64>]) -> (Vec<f64>, Vec<usize>) {
    let ndim = axes.len();
    let shape: Vec<usize> = axes.iter().map(|a| a.len()).collect();
    let total: usize = shape.iter().product();
    let mut out = vec![0.0f64; total];
    for (k, o) in out.iter_mut().enumerate() {
        let p: Vec<f64> = (0..ndim)
            .map(|d| {
                let mut idx = k;
                for dd in (d + 1)..ndim {
                    idx /= shape[dd];
                }
                axes[d][idx % shape[d]]
            })
            .collect();
        let mut v = f.trend_c;
        for d in 0..ndim {
            v += f.trend_a[d] * p[d];
        }
        for lat in &f.lattices {
            v += lat.at(&p);
        }
        *o = v;
    }
    (out, shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Плоскость восстанавливается трендом, остаток — ноль.
    #[test]
    fn plane_is_trend() {
        let pts = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let vals = [3.0, 4.0, 5.0, 6.0];
        let f = fit(&pts, &vals, None, None, &[2, 2], 1, None, "plane");
        let est = evaluate(&f, &pts, 2);
        for k in 0..4 {
            assert!(
                (est[k] - vals[k]).abs() < 1e-10,
                "est[{k}]={} want {}",
                est[k],
                vals[k]
            );
        }
    }

    /// Гауссов холм восстанавливается уровнями.
    #[test]
    fn hill_by_levels() {
        let n = 100;
        let mut pts = Vec::with_capacity(n * 2);
        let mut vals = Vec::with_capacity(n);
        for k in 0..n {
            let x = -1.0 + 2.0 * (k as f64 / n as f64);
            let y = -1.0 + 2.0 * ((k * 37 % n) as f64 / n as f64);
            let z = (-(x * x) - y * y).exp();
            pts.push(x);
            pts.push(y);
            vals.push(z);
        }
        let f = fit(&pts, &vals, None, None, &[4, 4], 6, None, "plane");
        let est = evaluate(&f, &pts, 2);
        let rms: f64 = (0..n).map(|k| {
            let d = est[k] - vals[k];
            d * d
        }).sum::<f64>() / n as f64;
        assert!(rms.sqrt() < 0.15, "rms={}", rms.sqrt());
    }
}
