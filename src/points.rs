// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Точки со значениями, порт `grid_isolines/tools/points.py` модуля
//! (`read_points`). Значение - из поля или из Z геометрии. Пропуск
//! точек без значения и без геометрии, отсев или срезка ураганных проб,
//! усреднение совпадающих точек. После усреднения точки идут в порядке
//! сортировки координат, как у `np.unique` в модуле.

use serde_json::Value as J;

use crate::args;
use crate::numpy;
use crate::report::{Report, ToolError};

/// Границы отсева ураганных проб (`kb2d.clip_outliers`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Outliers {
    pub vmin: Option<f64>,
    pub vmax: Option<f64>,
    /// Перцентиль с обеих сторон, 0 - без отсева.
    pub pct: f64,
    /// Срезать к границам вместо отсева.
    pub cap: bool,
}

/// Прочитанные точки.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Points {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    pub vs: Vec<f64>,
    /// Номер объекта или значение поля `id_field` у каждой точки.
    pub ids: Vec<J>,
}

/// `_num` модуля: число, булево как число, текст числа, иначе ничего.
pub fn num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        J::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Координата точки: число, `null` - NaN (так JSON хранит NaN модуля).
fn coord(v: &J) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// `read_points(items, zfield, layer, report, ...)`.
pub fn read_points(
    items: &[J],
    zfield: &str,
    layer: &J,
    report: &mut Report,
    clip: Outliers,
    id_field: Option<&str>,
) -> Result<Points, ToolError> {
    let zfield = zfield.trim();
    let use_geom_z = zfield.is_empty();
    if !use_geom_z {
        if let Some(fields) = layer["fields"].as_array()
            && !fields.iter().any(|f| f.as_str() == Some(zfield))
        {
            return Err(ToolError::new(
                "Поле «%s» не найдено в слое. Выберите поле значения Z.",
                args![zfield],
            ));
        }
    } else {
        let hasz = layer["hasz"].as_bool().unwrap_or_else(|| {
            items.iter().any(|it| {
                it["xyz"]
                    .as_array()
                    .is_some_and(|x| x.len() > 2 && num(&x[2]).is_some_and(f64::is_finite))
            })
        });
        if !hasz {
            return Err(ToolError::new(
                "Поле значения не задано, а геометрия слоя без Z. Укажите \
                 поле или подайте слой с отметкой в геометрии.",
                args![],
            ));
        }
    }
    let mut p = Points::default();
    let mut skipped_geom = 0usize;
    let mut skipped_value = 0usize;
    for it in items {
        let Some(xyz) = it["xyz"].as_array() else {
            skipped_geom += 1;
            continue;
        };
        let v = if use_geom_z {
            match xyz.get(2).and_then(num) {
                Some(v) if !v.is_nan() => v,
                _ => {
                    skipped_value += 1;
                    continue;
                }
            }
        } else {
            match num(&it["attrs"][zfield]) {
                Some(v) => v,
                None => {
                    skipped_value += 1;
                    continue;
                }
            }
        };
        p.xs.push(xyz.first().map_or(f64::NAN, coord));
        p.ys.push(xyz.get(1).map_or(f64::NAN, coord));
        p.vs.push(v);
        p.ids.push(match id_field {
            Some(f) => it["attrs"][f].clone(),
            None => it["id"].clone(),
        });
    }
    if skipped_value > 0 || skipped_geom > 0 {
        let what = if zfield.is_empty() {
            "Z геометрии"
        } else {
            zfield
        };
        if skipped_geom > 0 {
            report.info(
                "Пропущено точек: %d без значения «%s» и %d без \
                 геометрии. Прочитано: %d.",
                args![skipped_value, what, skipped_geom, p.xs.len()],
            );
        } else {
            report.info(
                "Пропущено точек: %d без значения «%s». Прочитано: %d.",
                args![skipped_value, what, p.xs.len()],
            );
        }
    }
    if p.xs.len() < 2 {
        return Err(ToolError::new(
            "Недостаточно валидных точек с числовым значением.",
            args![],
        ));
    }
    clip_outliers(&mut p, clip, report)?;
    merge_coincident(&mut p, report);
    if p.xs.len() < 2 {
        return Err(ToolError::new(
            "После усреднения совпадающих точек осталось < 2 узлов.",
            args![],
        ));
    }
    Ok(p)
}

/// Отсев или срезка ураганных проб.
fn clip_outliers(p: &mut Points, c: Outliers, report: &mut Report) -> Result<(), ToolError> {
    if c.vmin.is_none() && c.vmax.is_none() && c.pct <= 0.0 {
        return Ok(());
    }
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    if c.pct > 0.0 && !p.vs.is_empty() {
        let q = c.pct.clamp(0.0, 49.0);
        lo = numpy::percentile(&p.vs, q);
        hi = numpy::percentile(&p.vs, 100.0 - q);
    }
    if let Some(v) = c.vmin {
        lo = v;
    }
    if let Some(v) = c.vmax {
        hi = v;
    }
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    if lo == f64::NEG_INFINITY && hi == f64::INFINITY {
        return Ok(());
    }
    if c.cap {
        let mut nch = 0usize;
        for v in &mut p.vs {
            let w = v.clamp(lo, hi);
            if w != *v {
                nch += 1;
            }
            *v = w;
        }
        report.info(
            "Ураганные пробы: срезано %d значений к [%.4g; %.4g].",
            args![nch, lo, hi],
        );
    } else {
        let keep: Vec<bool> = p.vs.iter().map(|v| *v >= lo && *v <= hi).collect();
        let ncut = keep.iter().filter(|k| !**k).count();
        let pick = |a: &[f64]| -> Vec<f64> {
            a.iter()
                .zip(&keep)
                .filter(|(_, k)| **k)
                .map(|(v, _)| *v)
                .collect()
        };
        p.xs = pick(&p.xs);
        p.ys = pick(&p.ys);
        p.vs = pick(&p.vs);
        p.ids = p
            .ids
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(v, _)| v.clone())
            .collect();
        report.info(
            "Ураганные пробы: удалено %d точек вне [%.4g; %.4g], осталось %d.",
            args![ncut, lo, hi, p.xs.len()],
        );
    }
    if p.xs.len() < 2 {
        return Err(ToolError::new(
            "После отсева ураганных проб осталось < 2 точек.",
            args![],
        ));
    }
    Ok(())
}

/// Усреднение точек, совпавших до `span * 1e-7`. Ключ - координаты,
/// округлённые к кратному допуска (половина - к чётному, как `np.round`),
/// порядок - сортировка ключей, как у `np.unique(axis=0)`.
fn merge_coincident(p: &mut Points, report: &mut Report) {
    let n = p.xs.len();
    let range = |a: &[f64]| {
        a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - a.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let span = range(&p.xs).max(range(&p.ys)).max(1e-9);
    let tol = span * 1e-7;
    let key: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            [
                (p.xs[i] / tol).round_ties_even() * tol,
                (p.ys[i] / tol).round_ties_even() * tol,
            ]
        })
        .collect();
    // устойчивая сортировка: первый в группе - первый по слою
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        key[a][0]
            .partial_cmp(&key[b][0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                key[a][1]
                    .partial_cmp(&key[b][1])
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let mut inv = vec![0usize; n];
    let mut first: Vec<usize> = Vec::new();
    for (pos, &i) in order.iter().enumerate() {
        if pos == 0 || key[i] != key[order[pos - 1]] {
            first.push(i);
        }
        inv[i] = first.len() - 1;
    }
    let m = first.len();
    if m == n {
        return;
    }
    let mut sums = vec![0.0; m];
    let mut cnts = vec![0.0; m];
    for i in 0..n {
        sums[inv[i]] += p.vs[i];
        cnts[inv[i]] += 1.0;
    }
    p.vs = sums.iter().zip(&cnts).map(|(s, c)| s / c).collect();
    p.xs = first.iter().map(|&i| key[i][0]).collect();
    p.ys = first.iter().map(|&i| key[i][1]).collect();
    p.ids = first.iter().map(|&i| p.ids[i].clone()).collect();
    report.info(
        "Совпадающих точек усреднено: %d (осталось %d)",
        args![n - m, m],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pts(v: &[(f64, f64, J)]) -> Vec<J> {
        v.iter()
            .enumerate()
            .map(|(i, (x, y, z))| json!({"id": i, "xyz": [x, y, null], "attrs": {"z": z}}))
            .collect()
    }

    #[test]
    fn reads_field_and_skips_empty() {
        let items = pts(&[
            (0.0, 0.0, json!(1.0)),
            (1.0, 0.0, json!(null)),
            (2.0, 0.0, json!("3,5")),
            (3.0, 0.0, json!("4")),
        ]);
        let mut r = Report::default();
        let p = read_points(
            &items,
            "z",
            &json!({"fields": ["z"]}),
            &mut r,
            Outliers::default(),
            None,
        )
        .unwrap_or_default();
        // "3,5" Python float не читает: пропуск, как у модуля
        assert_eq!(p.vs, vec![1.0, 4.0]);
        assert_eq!(
            r.lines[0].text(),
            "Пропущено точек: 2 без значения «z». Прочитано: 2."
        );
    }

    #[test]
    fn coincident_points_are_averaged_in_sorted_order() {
        let items = pts(&[
            (5.0, 0.0, json!(1.0)),
            (0.0, 0.0, json!(2.0)),
            (5.0, 0.0, json!(3.0)),
            (0.0, 9.0, json!(4.0)),
        ]);
        let mut r = Report::default();
        let p = read_points(&items, "z", &J::Null, &mut r, Outliers::default(), None)
            .unwrap_or_default();
        // усреднённые точки встают на округлённый ключ, как у модуля:
        // 5 / 9e-7 = 5555555.6 -> 5555556 * 9e-7
        assert_eq!(p.xs, vec![0.0, 0.0, 5_555_556.0 * (9.0 * 1e-7)]);
        assert_eq!(p.ys, vec![0.0, 9.0, 0.0]);
        assert_eq!(p.vs, vec![2.0, 4.0, 2.0]);
        assert_eq!(p.ids, vec![json!(1), json!(3), json!(0)]);
        assert_eq!(
            r.lines[0].text(),
            "Совпадающих точек усреднено: 1 (осталось 3)"
        );
    }

    #[test]
    fn refusals() {
        let mut r = Report::default();
        let items = pts(&[(0.0, 0.0, json!(1.0)), (1.0, 0.0, json!(2.0))]);
        let e = read_points(
            &items,
            "q",
            &json!({"fields": ["z"]}),
            &mut r,
            Outliers::default(),
            None,
        );
        assert_eq!(
            e.err().map(|e| e.text()).unwrap_or_default(),
            "Поле «q» не найдено в слое. Выберите поле значения Z."
        );
        let e = read_points(
            &items,
            "",
            &json!({"hasz": false}),
            &mut r,
            Outliers::default(),
            None,
        );
        assert!(e.is_err());
        let e = read_points(
            &items[..1],
            "z",
            &J::Null,
            &mut r,
            Outliers::default(),
            None,
        );
        assert_eq!(
            e.err().map(|e| e.text()).unwrap_or_default(),
            "Недостаточно валидных точек с числовым значением."
        );
    }

    #[test]
    fn outliers_cut_and_cap() {
        let items = pts(&[
            (0.0, 0.0, json!(1.0)),
            (1.0, 0.0, json!(2.0)),
            (2.0, 0.0, json!(3.0)),
            (3.0, 0.0, json!(100.0)),
        ]);
        let mut r = Report::default();
        let cut = Outliers {
            vmax: Some(10.0),
            ..Outliers::default()
        };
        let p = read_points(&items, "z", &J::Null, &mut r, cut, None).unwrap_or_default();
        assert_eq!(p.vs, vec![1.0, 2.0, 3.0]);
        assert_eq!(
            r.lines[0].text(),
            "Ураганные пробы: удалено 1 точек вне [-inf; 10], осталось 3."
        );
        let cap = Outliers {
            vmax: Some(10.0),
            cap: true,
            ..Outliers::default()
        };
        let p = read_points(&items, "z", &J::Null, &mut r, cap, None).unwrap_or_default();
        assert_eq!(p.vs, vec![1.0, 2.0, 3.0, 10.0]);
    }
}
