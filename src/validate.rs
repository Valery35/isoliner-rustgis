// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Проверка рельефа по горизонталям, часть `grid_isolines/validate_core.py`
//! модуля: сечение рельефа и деление отметок на построение и проверку.

/// Уникальные конечные отметки по возрастанию (`np.unique`).
fn unique(levels: &[f64]) -> Vec<f64> {
    let mut u: Vec<f64> = levels.iter().copied().filter(|v| v.is_finite()).collect();
    u.sort_by(f64::total_cmp);
    u.dedup_by(|a, b| a == b);
    u
}

/// Сечение рельефа: наименьшая разность соседних уникальных отметок
/// больше `tol` (`contour_interval`). `None` - отметок меньше двух.
pub fn contour_interval(levels: &[f64], tol: f64) -> Option<f64> {
    let u = unique(levels);
    if u.len() < 2 {
        return None;
    }
    u.windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > tol)
        .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |x| x.min(d))))
}

/// Отметки для построения и для проверки (`split_levels`): каждая N-я
/// уникальная отметка со сдвигом уходит в проверку, крайние - никогда.
pub fn split_levels(levels: &[f64], every: i64, offset: i64) -> (Vec<f64>, Vec<f64>) {
    let u = unique(levels);
    if u.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let every = every.max(2);
    let off = offset.rem_euclid(every);
    let n = u.len();
    let mut build = Vec::new();
    let mut check = Vec::new();
    for (k, &lv) in u.iter().enumerate() {
        let edge = n >= 3 && (k == 0 || k == n - 1);
        if !edge && (k as i64) % every == off {
            check.push(lv);
        } else {
            build.push(lv);
        }
    }
    (build, check)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_like_module() {
        let lv: Vec<f64> = (0..10).map(|k| 100.0 + 5.0 * f64::from(k)).collect();
        let mut all = lv.clone();
        all.extend(&lv[..3]);
        assert_eq!(contour_interval(&all, 1e-6), Some(5.0));
        let (b, c) = split_levels(&all, 4, 0);
        // 0 и 9 - края, в проверку идут 4 и 8
        assert_eq!(c, vec![120.0, 140.0]);
        assert_eq!(b.len(), 8);
        let (_, c) = split_levels(&all, 4, 1);
        assert_eq!(c, vec![105.0, 125.0]);
        assert_eq!(contour_interval(&[1.0], 1e-6), None);
    }
}
