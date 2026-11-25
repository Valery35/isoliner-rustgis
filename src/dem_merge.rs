// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.12 Врезка подробной поверхности в региональную.
//! Порт ядра `dem_merge.py` модуля QGIS.
//!
//! Два грида одной величины и зона врезки. Снимается систематическое
//! расхождение по кольцу перекрытия (медиана или плоскость), затем
//! поверхности смешиваются весом: внутри зоны единица, за буфером ноль,
//! в кольце плавный переход с нулевой производной на концах.

/// Вес детальной поверхности: 1 внутри, 0 за буфером, плавно в кольце.
/// Переход: 1 - t²(3 - 2t), нулевая производная на обоих концах.
pub fn blend_weights(mask: &[bool], rows: usize, cols: usize, width_px: f64) -> Vec<f64> {
    let n = rows * cols;
    if width_px <= 0.0 {
        return mask.iter().map(|&m| if m { 1.0 } else { 0.0 }).collect();
    }
    let dist = distance_to_mask(mask, rows, cols);
    dist.iter()
        .map(|&d| {
            let t = (d / width_px).clamp(0.0, 1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        })
        .collect()
}

/// Расстояние (в ячейках) от каждой точки до ближайшей True в маске.
/// Приближение: два прохода (вперёд и назад), как Chamfer 3-4.
pub fn distance_to_mask(mask: &[bool], rows: usize, cols: usize) -> Vec<f64> {
    let inf = f64::INFINITY;
    let mut dist = vec![inf; rows * cols];
    // Прямой проход: сверху-слева вниз-вправо.
    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if mask[k] {
                dist[k] = 0.0;
                continue;
            }
            let mut best = inf;
            if r > 0 {
                best = best.min(dist[(r - 1) * cols + c] + 1.0);
            }
            if c > 0 {
                best = best.min(dist[r * cols + c - 1] + 1.0);
            }
            if r > 0 && c > 0 {
                best = best.min(dist[(r - 1) * cols + c - 1] + 1.4142);
            }
            if r > 0 && c + 1 < cols {
                best = best.min(dist[(r - 1) * cols + c + 1] + 1.4142);
            }
            dist[k] = best;
        }
    }
    // Обратный проход: снизу-справа вверх-влево.
    for r in (0..rows).rev() {
        for c in (0..cols).rev() {
            let k = r * cols + c;
            if mask[k] {
                continue;
            }
            let mut best = dist[k];
            if r + 1 < rows {
                best = best.min(dist[(r + 1) * cols + c] + 1.0);
            }
            if c + 1 < cols {
                best = best.min(dist[r * cols + c + 1] + 1.0);
            }
            if r + 1 < rows && c + 1 < cols {
                best = best.min(dist[(r + 1) * cols + c + 1] + 1.4142);
            }
            if r + 1 < rows && c > 0 {
                best = best.min(dist[(r + 1) * cols + c - 1] + 1.4142);
            }
            dist[k] = best;
        }
    }
    dist
}

/// Кольцо вокруг маски заданной ширины (в ячейках): True снаружи маски
/// в пределах width_px от её края.
pub fn ring_mask(mask: &[bool], rows: usize, cols: usize, width_px: f64) -> Vec<bool> {
    if width_px <= 0.0 {
        return mask.to_vec();
    }
    let dist = distance_to_mask(mask, rows, cols);
    dist.iter()
        .zip(mask.iter())
        .map(|(&d, &m)| !m && d <= width_px)
        .collect()
}

/// Статистика невязки: fine - coarse там, где обе есть и where = True.
#[derive(Debug, Clone, Default)]
pub struct ResidualStats {
    pub n: usize,
    pub median: f64,
    pub mean: f64,
    pub p05: f64,
    pub p95: f64,
}

/// Процентиль отсортированного массива (линейная интерполяция).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = p / 100.0 * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    let t = idx - lo as f64;
    sorted[lo] + t * (sorted[hi] - sorted[lo])
}

/// Медиана.
fn median(v: &mut Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 0 {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    } else {
        v[n / 2]
    }
}

/// Невязка двух поверхностей в кольце.
pub fn residual_stats(
    fine: &[f64],
    coarse: &[f64],
    where_: &[bool],
) -> ResidualStats {
    let mut diffs: Vec<f64> = Vec::new();
    for k in 0..fine.len() {
        if where_[k] && fine[k].is_finite() && coarse[k].is_finite() {
            diffs.push(fine[k] - coarse[k]);
        }
    }
    if diffs.is_empty() {
        return ResidualStats::default();
    }
    let n = diffs.len();
    let med = median(&mut diffs.clone());
    let mean = crate::numpy::sum(&diffs) / n as f64;
    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ResidualStats {
        n,
        median: med,
        mean,
        p05: percentile(&diffs, 5.0),
        p95: percentile(&diffs, 95.0),
    }
}

/// Режим снятия расхождения.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShiftMode {
    /// Медиана по кольцу: одно число.
    Median,
    /// Плоскость МНК по кольцу.
    Plane,
    /// Не снимать.
    None,
}

/// Отчёт о сшивке.
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    pub stats: ResidualStats,
    pub mode: String,
    pub correction_min: f64,
    pub correction_max: f64,
    /// Ячеек кольца перекрытия.
    pub overlap: usize,
    /// Предупреждение: нет перекрытия.
    pub warning: Option<String>,
    /// Невязка после поправки.
    pub after: ResidualStats,
    /// Пустых ячеек в результате.
    pub holes: usize,
}

/// Сшивает две поверхности (одна форма, одна сетка, NaN — нет данных).
/// mask — зона врезки. width_px — ширина перехода в ячейках этой сетки.
pub fn merge(
    fine: &[f64],
    coarse: &[f64],
    mask: &[bool],
    rows: usize,
    cols: usize,
    width_px: f64,
    shift_mode: ShiftMode,
) -> (Vec<f64>, MergeReport) {
    let n = rows * cols;
    let mut rep = MergeReport::default();

    // Кольцо: снаружи маски, где обе поверхности есть.
    let ring = ring_mask(mask, rows, cols, width_px)
        .iter()
        .zip(0..n)
        .map(|(&r, k)| r && fine[k].is_finite() && coarse[k].is_finite())
        .collect::<Vec<bool>>();
    rep.overlap = ring.iter().filter(|&&r| r).count();

    // Поправка.
    let correction = match shift_mode {
        ShiftMode::None => vec![0.0; n],
        ShiftMode::Median => {
            rep.stats = residual_stats(fine, coarse, &ring);
            vec![rep.stats.median; n]
        }
        ShiftMode::Plane => {
            rep.stats = residual_stats(fine, coarse, &ring);
            // Плоскость по МНК с отсечкой выбросов.
            let mut pts: Vec<(f64, f64, f64)> = Vec::new();
            for k in 0..n {
                if ring[k] && fine[k].is_finite() && coarse[k].is_finite() {
                    pts.push((fine[k] - coarse[k], (k % cols) as f64, (k / cols) as f64));
                }
            }
            if pts.is_empty() {
                vec![0.0; n]
            } else {
                // Отсечение по процентилям 5/95.
                let mut vals: Vec<f64> = pts.iter().map(|p| p.0).collect();
                vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let lo = percentile(&vals, 5.0);
                let hi = percentile(&vals, 95.0);
                let kept: Vec<(f64, f64, f64)> =
                    pts.into_iter().filter(|p| p.0 >= lo && p.0 <= hi).collect();
                // МНК: z = a*x + b*y + c.
                let dim = 3;
                let mut ata = [[0.0f64; 3]; 3];
                let mut atb = [0.0f64; 3];
                for &(d, x, y) in &kept {
                    let row = [x, y, 1.0];
                    for i in 0..dim {
                        atb[i] += row[i] * d;
                        for j in 0..dim {
                            ata[i][j] += row[i] * row[j];
                        }
                    }
                }
                // Гаусс.
                for col in 0..dim {
                    let piv = ata[col][col];
                    if piv.abs() < 1e-30 {
                        continue;
                    }
                    for row_i in (col + 1)..dim {
                        let f = ata[row_i][col] / piv;
                        for j in col..dim {
                            ata[row_i][j] -= f * ata[col][j];
                        }
                        atb[row_i] -= f * atb[col];
                    }
                }
                let mut sol = [0.0f64; 3];
                for i in (0..dim).rev() {
                    let mut s = atb[i];
                    for j in (i + 1)..dim {
                        s -= ata[i][j] * sol[j];
                    }
                    if ata[i][i].abs() > 1e-30 {
                        sol[i] = s / ata[i][i];
                    }
                }
                // Применяем ко всему полю.
                let mut out = vec![0.0f64; n];
                for k in 0..n {
                    let x = (k % cols) as f64;
                    let y = (k / cols) as f64;
                    out[k] = sol[0] * x + sol[1] * y + sol[2];
                }
                out
            }
        }
    };

    rep.mode = match shift_mode {
        ShiftMode::None => "none".into(),
        ShiftMode::Median => "median".into(),
        ShiftMode::Plane => "plane".into(),
    };
    rep.correction_min = correction
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    rep.correction_max = correction
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);

    // Региональная с поправкой.
    let coarse_adj: Vec<f64> = (0..n).map(|k| coarse[k] + correction[k]).collect();

    // Невязка после поправки.
    rep.after = residual_stats(fine, &coarse_adj, &ring);

    // Предупреждение о пустом кольце.
    if rep.overlap == 0 {
        rep.warning = Some(
            "Подробная поверхность не выходит за маску врезки: полосы перекрытия нет, \
             поправка не снята. Расширьте маску внутрь участка или задайте буфер по \
             данным съёмки."
                .into(),
        );
    }

    // Веса.
    let w = blend_weights(mask, rows, cols, width_px);

    // Смешивание.
    let mut out = vec![f64::NAN; n];
    let mut holes = 0;
    for k in 0..n {
        let f = fine[k];
        let c = coarse_adj[k];
        let hf = f.is_finite();
        let hc = c.is_finite();
        if hf && hc {
            out[k] = w[k] * f + (1.0 - w[k]) * c;
        } else if hf {
            out[k] = f;
        } else if hc {
            out[k] = c;
        } else {
            holes += 1;
        }
    }
    rep.holes = holes;
    (out, rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Две плоскости с постоянным сдвигом: медиана его снимает.
    #[test]
    fn constant_shift() {
        let rows = 20;
        let cols = 20;
        let n = rows * cols;
        // fine — плоскость 10, coarse — плоскость 0 (сдвиг 10).
        let fine: Vec<f64> = vec![10.0; n];
        let coarse: Vec<f64> = vec![0.0; n];
        // Маска — центр 5x5.
        let mut mask = vec![false; n];
        for r in 7..12 {
            for c in 7..12 {
                mask[r * cols + c] = true;
            }
        }
        let (out, rep) = merge(&fine, &coarse, &mask, rows, cols, 4.0, ShiftMode::Median);
        assert!(rep.overlap > 0, "кольцо не пустое");
        assert!((rep.stats.median - 10.0).abs() < 1e-10, "медиана={}", rep.stats.median);
        // Внутри маски — 10.
        let k = 9 * cols + 9;
        assert!((out[k] - 10.0).abs() < 1e-10, "внутри={}", out[k]);
        // Далеко снаружи — 10 (поправка применена ко всему полю).
        let k2 = 0;
        assert!((out[k2] - 10.0).abs() < 0.5, "снаружи={}", out[k2]);
    }

    /// Одна поверхность: вес 1 внутри, 0 далеко снаружи.
    #[test]
    fn weights_transition() {
        let rows = 30;
        let cols = 30;
        let mut mask = vec![false; rows * cols];
        for r in 10..20 {
            for c in 10..20 {
                mask[r * cols + c] = true;
            }
        }
        let w = blend_weights(&mask, rows, cols, 5.0);
        // Центр — 1.
        assert!((w[15 * cols + 15] - 1.0).abs() < 1e-10);
        // Далеко — 0.
        assert!(w[0].abs() < 1e-10);
        // Между — плавный переход (2 ячейки от края, t=0.4).
        let mid = w[15 * cols + 22];
        assert!(mid > 0.0 && mid < 1.0, "mid={}", mid);
    }
}
