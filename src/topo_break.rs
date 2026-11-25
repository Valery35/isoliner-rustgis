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

#[cfg(test)]
mod tests {
    use super::*;

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
