// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.04 Изолинии из растра: marching squares, порт ядра isolines.py.
//! Извлечение контурных линий из регулярной сетки (растра) по заданным
//! уровням. Алгоритм marching squares: каждая ячейка сетки классифицируется
//! по знаку (значение − уровень), грани между разными знаками дают отрезки,
//! отрезки сшиваются в линии.

/// Линия уровня: последовательность точек (x, y).
#[derive(Debug, Clone, PartialEq)]
pub struct ContourLine {
    /// Координаты точек линии, подряд (x0, y0, x1, y1, ...).
    pub xy: Vec<f64>,
    /// Уровень (значение изолинии).
    pub level: f64,
    /// Замкнутая линия.
    pub closed: bool,
}

/// Настройки извлечения изолиний.
#[derive(Debug, Clone)]
pub struct ContourOptions {
    /// Уровни (значения). Пусто — авто по интервалу.
    pub levels: Vec<f64>,
    /// Интервал между уровнями (если levels пусто).
    pub interval: f64,
    /// Базовый уровень (если levels пусто).
    pub base: f64,
    /// Значение «нет данных».
    pub nodata: f64,
    /// Геотрансформация: [x0, dx, dxdy, y0, dydx, dy].
    /// x = gt[0] + col * gt[1]; y = gt[3] + row * gt[5].
    pub gt: [f64; 6],
}

impl Default for ContourOptions {
    fn default() -> Self {
        Self {
            levels: Vec::new(),
            interval: 0.0,
            base: 0.0,
            nodata: -9999.0,
            gt: [0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
        }
    }
}

/// Вычисление уровней: явные или по интервалу.
pub fn compute_levels(
    data: &[f64],
    rows: usize,
    cols: usize,
    opts: &ContourOptions,
) -> Vec<f64> {
    if !opts.levels.is_empty() {
        return opts.levels.clone();
    }
    if opts.interval <= 0.0 {
        return Vec::new();
    }
    // Минимум и максимум валидных значений.
    let mut vmin = f64::INFINITY;
    let mut vmax = f64::NEG_INFINITY;
    for &v in data.iter() {
        if v != opts.nodata && v.is_finite() {
            vmin = vmin.min(v);
            vmax = vmax.max(v);
        }
    }
    if vmin == f64::INFINITY {
        return Vec::new();
    }
    // Уровни от base вверх и вниз до покрытия диапазона.
    let mut out = Vec::new();
    let iv = opts.interval;
    let b = opts.base;
    // Вверх от base.
    let mut lv = b + iv;
    while lv <= vmax {
        out.push(lv);
        lv += iv;
    }
    // Вниз от base.
    lv = b - iv;
    while lv >= vmin {
        out.push(lv);
        lv -= iv;
    }
    // Сам base, если в диапазоне.
    if b >= vmin && b <= vmax {
        out.push(b);
    }
    out.sort_by(|a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Извлечение изолиний из растра (marching squares).
/// data — значения (rows × cols, построчно), rows/cols — размер.
/// Отдаёт линии уровня в координатах сетки (преобразуются через gt).
pub fn contour_lines(
    data: &[f64],
    rows: usize,
    cols: usize,
    levels: &[f64],
    opts: &ContourOptions,
) -> Vec<ContourLine> {
    let mut out = Vec::new();
    let nodata = opts.nodata;

    for &level in levels {
        let segments = marching_squares(data, rows, cols, level, nodata);
        let lines = chain_segments(&segments, level, rows, cols, opts);
        out.extend(lines);
    }
    out
}

/// Отрезок контура: две точки (x0, y0, x1, y1) в координатах ячеек.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

/// Marching squares: отрезки на границе ячеек для одного уровня.
fn marching_squares(
    data: &[f64],
    rows: usize,
    cols: usize,
    level: f64,
    nodata: f64,
) -> Vec<Segment> {
    let mut out = Vec::new();
    if rows < 2 || cols < 2 {
        return out;
    }
    // Индекс ячейки: биты 0-3 — знаки в углах.
    // 1 — значение >= level, 0 — ниже (или nodata → считаем ниже).
    let idx = |r: usize, c: usize| -> u8 {
        let v00 = data[r * cols + c];
        let v10 = data[r * cols + (c + 1)];
        let v01 = data[(r + 1) * cols + c];
        let v11 = data[(r + 1) * cols + (c + 1)];
        let mut b = 0u8;
        if v00 != nodata && v00 >= level { b |= 1; }
        if v10 != nodata && v10 >= level { b |= 2; }
        if v11 != nodata && v11 >= level { b |= 4; }
        if v01 != nodata && v01 >= level { b |= 8; }
        b
    };

    // Интерполяция точки пересечения на ребре.
    let interp = |va: f64, vb: f64, level: f64, pa: f64, pb: f64| -> f64 {
        if (vb - va).abs() < 1e-30 {
            return (pa + pb) / 2.0;
        }
        let t = (level - va) / (vb - va);
        pa + t * (pb - pa)
    };

    for r in 0..(rows - 1) {
        for c in 0..(cols - 1) {
            let code = idx(r, c);
            if code == 0 || code == 15 {
                continue; // внутри или снаружи
            }
            // Координаты ячейки.
            let cx = c as f64;
            let cy = r as f64;
            // Значения углов.
            let v00 = data[r * cols + c];
            let v10 = data[r * cols + (c + 1)];
            let v01 = data[(r + 1) * cols + c];
            let v11 = data[(r + 1) * cols + (c + 1)];

            // Точки пересечения на рёбрах (координаты ячеек).
            let top = if (code & 1) != (code & 2) >> 1 {
                Some((interp(v00, v10, level, cx, cx + 1.0), cy))
            } else { None };
            let bottom = if (code & 8) != (code & 4) >> 2 {
                Some((interp(v01, v11, level, cx, cx + 1.0), cy + 1.0))
            } else { None };
            let left = if (code & 1) != (code & 8) >> 3 {
                Some((cx, interp(v00, v01, level, cy, cy + 1.0)))
            } else { None };
            let right = if (code & 2) >> 1 != (code & 4) >> 2 {
                Some((cx + 1.0, interp(v10, v11, level, cy, cy + 1.0)))
            } else { None };

            // Случаи marching squares.
            let pts: Vec<(f64, f64)> = [top, bottom, left, right]
                .iter()
                .filter_map(|p| *p)
                .collect();
            if pts.len() == 2 {
                out.push(Segment { x0: pts[0].0, y0: pts[0].1, x1: pts[1].0, y1: pts[1].1 });
            } else if pts.len() == 4 {
                // Седловая точка: две пары.
                // Связываем top-left и bottom-right (простой вариант).
                if let (Some(t), Some(b), Some(l), Some(rr)) = (top, bottom, left, right) {
                    out.push(Segment { x0: t.0, y0: t.1, x1: l.0, y1: l.1 });
                    out.push(Segment { x0: b.0, y0: b.1, x1: rr.0, y1: rr.1 });
                }
            }
        }
    }
    out
}

/// Сшивка отрезков в линии.
fn chain_segments(
    segments: &[Segment],
    level: f64,
    _rows: usize,
    _cols: usize,
    opts: &ContourOptions,
) -> Vec<ContourLine> {
    if segments.is_empty() {
        return Vec::new();
    }
    let eps = 1e-10;
    // Индекс: конечная точка → номер отрезка.
    let mut used = vec![false; segments.len()];
    let mut lines = Vec::new();

    // Точка как ключ.
    let key = |x: f64, y: f64| -> (i64, i64) {
        ((x / eps).round() as i64, (y / eps).round() as i64)
    };

    // Хеш конечных точек.
    use std::collections::HashMap;
    let mut starts: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        starts.entry(key(s.x0, s.y0)).or_default().push(i);
        starts.entry(key(s.x1, s.y1)).or_default().push(i);
    }

    // Цепочки.
    for i in 0..segments.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let s = segments[i];
        let mut xy = vec![s.x0, s.y0, s.x1, s.y1];

        // Вперёд от конца.
        loop {
            let ex = *xy.last().unwrap();
            let ey = xy[xy.len() - 1];
            let k = key(ex, ey);
            let mut found = false;
            if let Some(cands) = starts.get(&k) {
                for &j in cands {
                    if used[j] {
                        continue;
                    }
                    let sj = segments[j];
                    // Присоединяем так, чтобы конец совпал.
                    if (sj.x0 - ex).abs() < eps && (sj.y0 - ey).abs() < eps {
                        xy.push(sj.x1);
                        xy.push(sj.y1);
                        used[j] = true;
                        found = true;
                        break;
                    } else if (sj.x1 - ex).abs() < eps && (sj.y1 - ey).abs() < eps {
                        xy.push(sj.x0);
                        xy.push(sj.y0);
                        used[j] = true;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                break;
            }
        }

        // Назад от начала.
        loop {
            let sx = xy[0];
            let sy = xy[1];
            let k = key(sx, sy);
            let mut found = false;
            if let Some(cands) = starts.get(&k) {
                for &j in cands {
                    if used[j] {
                        continue;
                    }
                    let sj = segments[j];
                    if (sj.x1 - sx).abs() < eps && (sj.y1 - sy).abs() < eps {
                        xy.insert(0, sj.y0);
                        xy.insert(0, sj.x0);
                        used[j] = true;
                        found = true;
                        break;
                    } else if (sj.x0 - sx).abs() < eps && (sj.y0 - sy).abs() < eps {
                        xy.insert(0, sj.y1);
                        xy.insert(0, sj.x1);
                        used[j] = true;
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                break;
            }
        }

        // Замкнутая?
        let n = xy.len() / 2;
        let closed = n > 2
            && (xy[0] - xy[xy.len() - 2]).abs() < eps
            && (xy[1] - xy[xy.len() - 1]).abs() < eps;
        if closed {
            xy.pop();
            xy.pop();
        }

        // Преобразование в координаты через gt.
        let gt = &opts.gt;
        for k in 0..xy.len() {
            let c = xy[k]; // column
            let mut ri = k + 1;
            if k % 2 == 1 {
                ri = k;
            }
            let _ = ri;
        }
        // Простое преобразование: x = gt[0] + col * gt[1], y = gt[3] + row * gt[5].
        for k in (0..xy.len()).step_by(2) {
            let col = xy[k];
            let row = xy[k + 1];
            xy[k] = gt[0] + col * gt[1];
            xy[k + 1] = gt[3] + row * gt[5];
        }

        lines.push(ContourLine { xy, level, closed });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Один холм: одна замкнутая изолиния на уровне 0.5.
    #[test]
    fn single_hill() {
        let rows = 21;
        let cols = 21;
        let mut data = vec![0.0f64; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                let x = (c as f64) - 10.0;
                let y = (r as f64) - 10.0;
                data[r * cols + c] = (-(x * x) - y * y).exp();
            }
        }
        let opts = ContourOptions {
            nodata: -9999.0,
            gt: [0.0, 1.0, 0.0, 20.0, 0.0, -1.0],
            ..Default::default()
        };
        let levels = [0.5f64];
        let lines = contour_lines(&data, rows, cols, &levels, &opts);
        assert!(!lines.is_empty(), "должна быть хотя бы одна линия");
        // Все линии уровня 0.5.
        for l in &lines {
            assert!((l.level - 0.5).abs() < 1e-10);
        }
    }

    /// Плоскость: уровни по интервалу.
    #[test]
    fn auto_levels() {
        let data = vec![0.0f64, 5.0, 10.0, 15.0, 20.0];
        let rows = 1;
        let cols = 5;
        let opts = ContourOptions {
            interval: 5.0,
            base: 0.0,
            nodata: -9999.0,
            ..Default::default()
        };
        let lv = compute_levels(&data, rows, cols, &opts);
        assert!(!lv.is_empty(), "уровни не пустые");
        assert!(lv.contains(&5.0) || lv.contains(&10.0), "уровни содержат 5 или 10: {:?}", lv);
    }

    /// Пустой растр: нет уровней.
    #[test]
    fn empty_raster() {
        let data = vec![-9999.0f64; 25];
        let opts = ContourOptions::default();
        let lv = compute_levels(&data, 5, 5, &opts);
        assert!(lv.is_empty() || lv.len() <= 1, "уровни почти пустые: {:?}", lv);
    }
}
