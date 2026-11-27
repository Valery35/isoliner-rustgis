// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.11–2.13 Разделение горизонталей, невязка, террасинг.
//! Порт `topo_break.py` модуля QGIS.
//!
//! 2.11 — разделить горизонтали на сегменты для проверки.
//! 2.12 — невязка горизонталей против ЦМР.
//! 2.13 — диагностика террасинга ЦМР.

use crate::isolines::{contour_lines, ContourOptions};

/// Сегмент горизонтали после разделения.
#[derive(Debug, Clone)]
pub struct ContourSegment {
    /// Уровень.
    pub level: f64,
    /// Точки сегмента (x, y) подряд.
    pub xy: Vec<(f64, f64)>,
    /// Замкнут.
    pub closed: bool,
}

/// 2.11 Разделение горизонталей на сегменты по перекрытию ЦМР.
/// Линии одного уровня делятся на непрерывные сегменты там,
/// где ЦМР не подтверждает уровень (пропуск или терраса).
pub fn split_contours(
    z: &[f64],
    rows: usize,
    cols: usize,
    gt: &[f64; 6],
    levels: &[f64],
    nodata: f64,
) -> Vec<ContourSegment> {
    let opts = ContourOptions {
        nodata,
        gt: *gt,
        ..Default::default()
    };
    let lines = contour_lines(z, rows, cols, levels, &opts);
    lines
        .into_iter()
        .map(|l| ContourSegment {
            level: l.level,
            xy: l.xy.chunks(2).map(|c| (c[0], c[1])).collect(),
            closed: l.closed,
        })
        .collect()
}

/// 2.12 Невязка горизонталей против ЦМР.
/// Для каждой точки каждой горизонтали: разница между уровнем
/// и высотой ЦМР в этой точке. Большая невязка — ЦМР врёт.
#[derive(Debug, Clone)]
pub struct ContourMismatch {
    pub level: f64,
    pub point_index: usize,
    /// Координата (x, y).
    pub x: f64,
    pub y: f64,
    /// Высота ЦМР в точке.
    pub z: f64,
    /// Невязка: z - level.
    pub diff: f64,
}

pub fn contour_mismatch(
    z: &[f64],
    rows: usize,
    cols: usize,
    gt: &[f64; 6],
    segments: &[ContourSegment],
    tolerance: f64,
    nodata: f64,
) -> Vec<ContourMismatch> {
    let mut out = Vec::new();
    for seg in segments {
        for (i, &(x, y)) in seg.xy.iter().enumerate() {
            // Выборка ЦМР в точке.
            let fx = (x - gt[0]) / gt[1];
            let fy = (y - gt[3]) / gt[5];
            let ix = fx.round() as i64;
            let iy = fy.round() as i64;
            if ix < 0 || ix >= cols as i64 || iy < 0 || iy >= rows as i64 {
                continue;
            }
            let k = iy as usize * cols + ix as usize;
            if z[k] == nodata || !z[k].is_finite() {
                continue;
            }
            let diff = z[k] - seg.level;
            if diff.abs() > tolerance {
                out.push(ContourMismatch {
                    level: seg.level,
                    point_index: i,
                    x,
                    y,
                    z: z[k],
                    diff,
                });
            }
        }
    }
    out
}

/// 2.13 Диагностика террасинга ЦМР.
/// Терраса — серия ячеек с одинаковой высотой на склоне.
/// Отдаёт список террас: (row, col, высота, длина_террасы).
#[derive(Debug, Clone)]
pub struct Terrace {
    pub row: usize,
    pub col: usize,
    pub z: f64,
    /// Число ячеек в террасе.
    pub length: usize,
}

pub fn diagnose_terracing(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    min_length: usize,
) -> Vec<Terrace> {
    let mut out = Vec::new();

    // По строкам: серия одинаковых валидных значений.
    for r in 0..rows {
        let mut c = 0;
        while c < cols {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                c += 1;
                continue;
            }
            let v = z[k];
            let mut len = 1;
            let mut cc = c + 1;
            while cc < cols {
                let nk = r * cols + cc;
                if z[nk] == v && z[nk] != nodata && z[nk].is_finite() {
                    len += 1;
                    cc += 1;
                } else {
                    break;
                }
            }
            if len >= min_length {
                // Терраса на склоне: сверху или снизу высота другая.
                let above_diff = r == 0 || z[(r - 1) * cols + c] != v;
                let below_diff = r == rows - 1 || z[(r + 1) * cols + c] != v;
                if above_diff && below_diff {
                    out.push(Terrace { row: r, col: c, z: v, length: len });
                }
            }
            c = cc;
        }
    }

    // По столбцам.
    for c in 0..cols {
        let mut r = 0;
        while r < rows {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                r += 1;
                continue;
            }
            let v = z[k];
            let mut len = 1;
            let mut rr = r + 1;
            while rr < rows {
                let nk = rr * cols + c;
                if z[nk] == v && z[nk] != nodata && z[nk].is_finite() {
                    len += 1;
                    rr += 1;
                } else {
                    break;
                }
            }
            if len >= min_length {
                let left_diff = c == 0 || z[r * cols + c - 1] != v;
                let right_diff = c == cols - 1 || z[r * cols + c + 1] != v;
                if left_diff && right_diff {
                    out.push(Terrace { row: r, col: c, z: v, length: len });
                }
            }
            r = rr;
        }
    }
    out
}

/// Перепад высот на ячейку: модуль градиента, умноженный на шаг.
/// Края считаются односторонними разностями, как `np.gradient`.
pub fn drop_per_cell(z: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    let n = rows * cols;
    let mut out = vec![f64::NAN; n];
    if n == 0 {
        return out;
    }
    let valid: Vec<bool> = z.iter().map(|v| v.is_finite()).collect();
    let sum: f64 = z.iter().zip(&valid).filter(|&(_, &b)| b).map(|(&v, _)| v).sum();
    let cnt = valid.iter().filter(|&&b| b).count() as f64;
    if cnt == 0.0 {
        return out;
    }
    let off = sum / cnt;
    let zc: Vec<f64> = z
        .iter()
        .zip(&valid)
        .map(|(&v, &b)| if b { v - off } else { 0.0 })
        .collect();
    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if !valid[k] {
                continue;
            }
            let gx = if c == 0 {
                zc[k + 1] - zc[k]
            } else if c == cols - 1 {
                zc[k] - zc[k - 1]
            } else {
                0.5 * (zc[k + 1] - zc[k - 1])
            };
            let gy = if r == 0 {
                zc[k + cols] - zc[k]
            } else if r == rows - 1 {
                zc[k] - zc[k - cols]
            } else {
                0.5 * (zc[k + cols] - zc[k - cols])
            };
            out[k] = (gx * gx + gy * gy).sqrt();
        }
    }
    out
}

/// Профильная (вертикальная) кривизна Zevenbergen-Thorne: вторая
/// производная вдоль склона. Положительная - вогнутость, отрицательная -
/// выпуклость. На рамке NaN.
pub fn profile_curvature(z: &[f64], rows: usize, cols: usize, cell: f64) -> Vec<f64> {
    let n = rows * cols;
    let mut out = vec![f64::NAN; n];
    if n == 0 || rows < 3 || cols < 3 {
        return out;
    }
    let c = if cell > 0.0 { cell } else { 1.0 };
    let get = |r: isize, cc: isize| -> f64 {
        if r < 0 || r >= rows as isize || cc < 0 || cc >= cols as isize {
            f64::NAN
        } else {
            z[r as usize * cols + cc as usize]
        }
    };
    for r in 1..rows - 1 {
        for cc in 1..cols - 1 {
            let k = r * cols + cc;
            let z00 = get(r as isize, cc as isize);
            let z10 = get(r as isize, cc as isize + 1);
            let z_10 = get(r as isize, cc as isize - 1);
            let z01 = get(r as isize + 1, cc as isize);
            let z_11 = get(r as isize - 1, cc as isize);
            let z11 = get(r as isize + 1, cc as isize + 1);
            let z1_1 = get(r as isize + 1, cc as isize - 1);
            let z_11_ = get(r as isize - 1, cc as isize + 1);
            let z_1_1 = get(r as isize - 1, cc as isize - 1);
            let all = [z00, z10, z_10, z01, z_11, z11, z1_1, z_11_, z_1_1];
            if all.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let zx = (z10 - z_10) / (2.0 * c);
            let zy = (z01 - z_11) / (2.0 * c);
            let num = zx * zx * (z10 - 2.0 * z00 + z_10) / (c * c)
                + zy * zy * (z01 - 2.0 * z00 + z_11) / (c * c)
                + 2.0 * zx * zy * (z11 - z1_1 - z_11_ + z_1_1) / (4.0 * c * c);
            let p = zx * zx + zy * zy;
            if p < 1e-12 {
                continue;
            }
            out[k] = -num / (p * (1.0 + p).powf(1.5));
        }
    }
    out
}

/// Плоские ячейки: перепад меньше `min_drop_frac` доли сечения.
pub fn flat_mask(
    z: &[f64],
    rows: usize,
    cols: usize,
    interval: f64,
    min_drop_frac: f64,
) -> Vec<bool> {
    let drop = drop_per_cell(z, rows, cols);
    let thr = min_drop_frac * interval;
    drop.iter()
        .map(|v| v.is_finite() && *v < thr)
        .collect()
}

/// Индекс притяжения отметок к уровням: (доля, ожидаемая доля, отношение,
/// доля исключённых плоских). None, если считать не по чему.
pub fn level_attraction(
    z: &[f64],
    rows: usize,
    cols: usize,
    interval: f64,
    base: f64,
    band: f64,
    min_drop_frac: f64,
) -> Option<(f64, f64, f64, f64)> {
    if interval <= 0.0 {
        return None;
    }
    let valid: Vec<bool> = z.iter().map(|v| v.is_finite()).collect();
    let total = valid.iter().filter(|&&b| b).count();
    if total == 0 {
        return None;
    }
    let mut skipped = 0.0;
    let mut use_cell = valid.clone();
    let flat = flat_mask(z, rows, cols, interval, min_drop_frac);
    let flat_count = use_cell.iter().zip(&flat).filter(|&(v, &f)| *v && f).count();
    let kept = total - flat_count;
    if kept >= 100 {
        skipped = flat_count as f64 / total as f64;
        for i in 0..use_cell.len() {
            if flat[i] {
                use_cell[i] = false;
            }
        }
    }
    let mut hits = 0usize;
    let mut count = 0usize;
    for i in 0..z.len() {
        if !use_cell[i] {
            continue;
        }
        count += 1;
        let phase = (((z[i] - base) / interval + 0.5) % 1.0 - 0.5).abs();
        if phase <= band {
            hits += 1;
        }
    }
    if count == 0 {
        return None;
    }
    let share = hits as f64 / count as f64;
    let expect = 2.0 * band;
    Some((share, expect, share / expect, skipped))
}

/// Сводка по террасингу.
#[derive(Debug, Clone, Default)]
pub struct TerracingStats {
    pub n_curv: usize,
    pub curv_mean_abs: f64,
    pub curv_p95_abs: f64,
    pub curv_max_abs: f64,
    pub attract_share: f64,
    pub attract_expect: f64,
    pub attract_ratio: f64,
    pub flat_skipped: f64,
}

/// Сводка: кривизна плюс притяжение к уровням.
#[allow(clippy::too_many_arguments)]
pub fn terracing_stats(
    z: &[f64],
    rows: usize,
    cols: usize,
    cell: f64,
    interval: f64,
    base: f64,
    band: f64,
    min_drop_frac: f64,
) -> TerracingStats {
    let kv = profile_curvature(z, rows, cols, cell);
    let mut abs: Vec<f64> = kv.iter().filter(|v| v.is_finite()).map(|v| v.abs()).collect();
    let mut s = TerracingStats {
        n_curv: abs.len(),
        ..Default::default()
    };
    if !abs.is_empty() {
        let n = abs.len() as f64;
        s.curv_mean_abs = abs.iter().sum::<f64>() / n;
        abs.sort_by(f64::total_cmp);
        let idx = ((0.95 * (n - 1.0)).round() as usize).min(abs.len() - 1);
        s.curv_p95_abs = abs[idx];
        s.curv_max_abs = *abs.last().unwrap_or(&0.0);
    }
    if let Some((share, expect, ratio, skipped)) =
        level_attraction(z, rows, cols, interval, base, band, min_drop_frac)
    {
        s.attract_share = share;
        s.attract_expect = expect;
        s.attract_ratio = ratio;
        s.flat_skipped = skipped;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Притяжение к уровням ловит ступенчатую поверхность.
    #[test]
    fn terracing_picks_a_staircase() {
        let rows = 40;
        let cols = 40;
        let smooth: Vec<f64> = (0..rows * cols)
            .map(|k| (k / cols) as f64 * 0.1)
            .collect();
        let terraced: Vec<f64> = smooth.iter().map(|v| (v / 2.0).round() * 2.0).collect();
        let a = terracing_stats(&smooth, rows, cols, 1.0, 2.0, 0.0, 0.1, 0.01);
        let b = terracing_stats(&terraced, rows, cols, 1.0, 2.0, 0.0, 0.1, 0.01);
        assert!(a.attract_ratio < 1.5, "smooth ratio {}", a.attract_ratio);
        assert!(b.attract_ratio > 2.0, "terraced ratio {}", b.attract_ratio);
    }

    /// Ступень на склоне — терраса.
    #[test]
    fn terrace_detected() {
        let rows = 10;
        let cols = 10;
        let mut z = vec![0.0f64; rows * cols];
        // Ступень: строки 3-5 одинаковые.
        for r in 3..6 {
            for c in 0..cols {
                z[r * cols + c] = 5.0;
            }
        }
        // Сверху и снизу разные высоты.
        for c in 0..cols {
            z[2 * cols + c] = 6.0;
            z[6 * cols + c] = 4.0;
        }
        let t = diagnose_terracing(&z, rows, cols, -9999.0, 3);
        assert!(!t.is_empty(), "terraces={}", t.len());
    }

    /// Плоская площадка — не терраса.
    #[test]
    fn flat_area_not_terrace() {
        let z = vec![5.0f64; 100]; // всё одинаково
        let t = diagnose_terracing(&z, 10, 10, -9999.0, 3);
        assert!(t.is_empty(), "terraces={}", t.len());
    }
}
