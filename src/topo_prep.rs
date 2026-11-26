// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.04 Подготовка рельефа: заполнение понижений и сглаживание.
//! Порт `topo_smooth.py` модуля QGIS.

/// Заполнение понижений (priority-flood).
/// Каждая ячейка приподнимается до минимального уровня, при котором
/// из неё есть путь стока к краю растра.
pub fn fill_depressions(
    z: &mut [f64],
    rows: usize,
    cols: usize,
    nodata: f64,
) {
    let n = rows * cols;
    if n == 0 {
        return;
    }

    // Приоритетная очередь (бинарная куча): (высота, индекс).
    let mut visited = vec![false; n];
    let mut heap: Vec<(f64, usize)> = Vec::with_capacity(n);

    // Край заливается первым.
    for r in 0..rows {
        for c in 0..cols {
            if r == 0 || r == rows - 1 || c == 0 || c == cols - 1 {
                let k = r * cols + c;
                if z[k] != nodata && z[k].is_finite() {
                    heap.push((z[k], k));
                    visited[k] = true;
                }
            }
        }
    }
    heap.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    const OFF: [(isize, isize); 8] = [
        (-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1),
    ];

    while let Some((zh, k)) = heap_pop(&mut heap) {
        let r = k / cols;
        let c = k % cols;
        for &(dr, dc) in OFF.iter() {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                continue;
            }
            let nk = nr as usize * cols + nc as usize;
            if visited[nk] || z[nk] == nodata || !z[nk].is_finite() {
                continue;
            }
            visited[nk] = true;
            // Ячейка приподнимается до уровня соседа, если она ниже.
            if z[nk] < zh {
                z[nk] = zh;
            }
            heap_push(&mut heap, (z[nk], nk));
        }
    }
}

/// Сглаживание с ограничением: гаусс с обрезкой выбросов.
/// sigma — радиус в ячейках, max_change — предел изменения высоты.
pub fn smooth_clamped(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    sigma: f64,
    max_change: f64,
) -> Vec<f64> {
    let n = rows * cols;
    let mut out = z.to_vec();
    if sigma <= 0.0 || n == 0 {
        return out;
    }

    let radius = (sigma * 2.0).ceil() as usize;
    // Гауссовы веса.
    let mut gw: Vec<Vec<f64>> = Vec::with_capacity(2 * radius + 1);
    for dr in -(radius as isize)..=(radius as isize) {
        let mut row = Vec::with_capacity(2 * radius + 1);
        for dc in -(radius as isize)..=(radius as isize) {
            let d = ((dr * dr + dc * dc) as f64).sqrt();
            row.push((-(d * d) / (2.0 * sigma * sigma)).exp());
        }
        gw.push(row);
    }

    for r in 0..rows {
        for c in 0..cols {
            let k = r * cols + c;
            if z[k] == nodata || !z[k].is_finite() {
                continue;
            }
            let mut sum = 0.0;
            let mut wsum = 0.0;
            for dr in -(radius as isize)..=(radius as isize) {
                for dc in -(radius as isize)..=(radius as isize) {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                        continue;
                    }
                    let nk = nr as usize * cols + nc as usize;
                    if z[nk] == nodata || !z[nk].is_finite() {
                        continue;
                    }
                    let w = gw[(dr + radius as isize) as usize][(dc + radius as isize) as usize];
                    sum += w * z[nk];
                    wsum += w;
                }
            }
            if wsum > 0.0 {
                let smooth = sum / wsum;
                let change = smooth - z[k];
                // Ограничение: изменение не больше max_change.
                let clamped = if change.abs() > max_change {
                    z[k] + max_change * change.signum()
                } else {
                    smooth
                };
                out[k] = clamped;
            }
        }
    }
    out
}

/// Снятие ступеней с ограничением, порт `topo_smooth.smooth_clamped`:
/// 4-соседнее сглаживание, каждая ячейка не уходит от исходной дальше
/// `band` долей сечения. Ограничение держится по исходной поверхности на
/// всех итерациях, поэтому горизонтали не сдвигаются.
pub fn smooth_clamped_terrace(
    z: &[f64],
    rows: usize,
    cols: usize,
    nodata: f64,
    interval: f64,
    iters: usize,
    band: f64,
) -> Vec<f64> {
    let n = rows * cols;
    if n == 0 || iters == 0 || interval <= 0.0 {
        return z.to_vec();
    }
    let valid: Vec<bool> = z
        .iter()
        .map(|&v| v.is_finite() && v != nodata)
        .collect();
    if !valid.iter().any(|&b| b) {
        return z.to_vec();
    }
    // Центрирование: отметки вокруг среднего, чтобы float32 не терял точность.
    let (sum, cnt) = z
        .iter()
        .zip(&valid)
        .filter(|&(_, &b)| b)
        .fold((0.0f64, 0usize), |(s, c), (&v, _)| (s + v, c + 1));
    let off = sum / cnt as f64;
    let z0: Vec<f64> = z
        .iter()
        .zip(&valid)
        .map(|(&v, &b)| if b { v - off } else { 0.0 })
        .collect();
    let delta = band * interval;
    let lo: Vec<f64> = z0.iter().map(|&v| v - delta).collect();
    let hi: Vec<f64> = z0.iter().map(|&v| v + delta).collect();
    let mut cur = z0.clone();
    let mut next = cur.clone();
    const OFF: [(isize, isize); 4] = [(-1, 0), (0, -1), (0, 1), (1, 0)];
    for _ in 0..iters {
        for r in 0..rows {
            for c in 0..cols {
                let k = r * cols + c;
                if !valid[k] {
                    next[k] = 0.0;
                    continue;
                }
                let mut acc = 4.0 * cur[k];
                let mut w = 4.0;
                for &(dr, dc) in &OFF {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                        continue;
                    }
                    let nk = nr as usize * cols + nc as usize;
                    if valid[nk] {
                        acc += cur[nk];
                        w += 1.0;
                    }
                }
                next[k] = (acc / w).clamp(lo[k], hi[k]);
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    z.iter()
        .zip(&valid)
        .zip(&cur)
        .map(|((&orig, &b), &v)| if b { v + off } else { orig })
        .collect()
}

/// Извлечение минимального элемента из кучи.
fn heap_pop(heap: &mut Vec<(f64, usize)>) -> Option<(f64, usize)> {
    if heap.is_empty() {
        return None;
    }
    let top = heap[0];
    let last = heap.pop()?;
    if !heap.is_empty() {
        heap[0] = last;
        sift_down(heap, 0);
    }
    Some(top)
}

/// Добавление элемента в кучу. Куча минимальная: priority-flood берёт
/// самую низкую ячейку первой.
fn heap_push(heap: &mut Vec<(f64, usize)>, item: (f64, usize)) {
    heap.push(item);
    let mut i = heap.len() - 1;
    while i > 0 {
        let parent = (i - 1) / 2;
        if heap[i].0 >= heap[parent].0 {
            break;
        }
        heap.swap(i, parent);
        i = parent;
    }
}

/// Просеивание вниз: наверх поднимается меньший из детей.
fn sift_down(heap: &mut [(f64, usize)], start: usize) {
    let n = heap.len();
    let mut i = start;
    loop {
        let left = 2 * i + 1;
        let right = 2 * i + 2;
        let mut best = i;
        if left < n && heap[left].0 < heap[best].0 {
            best = left;
        }
        if right < n && heap[right].0 < heap[best].0 {
            best = right;
        }
        if best == i {
            break;
        }
        heap.swap(i, best);
        i = best;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Котловина заполняется до уровня стока.
    #[test]
    fn fill_pit() {
        let rows = 7;
        let cols = 7;
        let mut z = vec![10.0f64; rows * cols];
        // Котловина в центре.
        z[3 * cols + 3] = 0.0;
        fill_depressions(&mut z, rows, cols, -9999.0);
        // Центр приподнят до уровня соседей.
        assert!(
            z[3 * cols + 3] >= 9.0,
            "pit filled to {}",
            z[3 * cols + 3]
        );
    }

    /// Снятие ступеней: выброс уменьшается, но не выходит за половину сечения.
    #[test]
    fn terrace_smooth_stays_within_band() {
        let rows = 5;
        let cols = 5;
        let mut z = vec![10.0f64; rows * cols];
        z[12] = 20.0;
        let out = smooth_clamped_terrace(&z, rows, cols, -9999.0, 4.0, 10, 0.5);
        assert!(out[12] < 20.0, "spike stayed {}", out[12]);
        assert!(out[12] >= 18.0 - 1e-9, "spike dropped to {}", out[12]);
        // Ограничение держится для всех ячеек: не дальше половины сечения
        // от собственного исходного значения.
        for (i, &v) in out.iter().enumerate() {
            assert!(
                (v - z[i]).abs() <= 2.0 + 1e-9,
                "cell {i} left the band: {v} vs {}",
                z[i]
            );
        }
    }

    /// Сглаживание: выброс уменьшается.
    #[test]
    fn smooth_outlier() {
        let rows = 5;
        let cols = 5;
        let z = vec![10.0f64; rows * cols];
        // Один выброс.
        let mut z2 = z.clone();
        z2[2 * cols + 2] = 100.0;
        let s = smooth_clamped(&z2, rows, cols, -9999.0, 1.0, 5.0);
        // Выброс уменьшился, но не более чем на max_change.
        assert!(
            s[2 * cols + 2] < 100.0 && s[2 * cols + 2] > 90.0,
            "smoothed={}",
            s[2 * cols + 2]
        );
    }
}
