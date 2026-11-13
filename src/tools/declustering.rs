// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.05 Декластеризация (веса), порт `grid_isolines/tools/declustering.py`.
//!
//! Поля - ключи класса `DeclusteringAlgorithm`: INPUT (точки), ZFIELD,
//! MODE (0 авто, 1 ручной), CELL_SIZE, OBJECTIVE (0 минимум среднего,
//! 1 максимум), NCELL, ASPECT, NOFF. Выход OUTPUT - точки с полем
//! значения и полем `wt`. HTML-отчёт модуля здесь не пишется, его
//! цифры идут в журнал теми же строками.

use serde_json::{Value as J, json};

use crate::args;
use crate::declus;
use crate::field_format::{plan_for, row};

/// Подписи полей выхода (`FIELD_LABELS` класса), по ним - точность.
pub const FIELD_LABELS: &[(&str, &str)] = &[("wt", "Вес декластеризации")];
use crate::points::{Outliers, read_points};
use crate::report::{Report, ToolError};
use crate::tools::Values;

/// Имя поля значения на выходе, как `value_field` модуля: всё, что не
/// буква, цифра или `_`, - в `_`, края без `_`, не длиннее 40 знаков.
pub fn value_field(zfield: &str) -> String {
    let s = san(zfield);
    if s.is_empty() { "z".into() } else { s }
}

/// Имя поля из текста (`_san`): всё, что не буква, цифра или `_`, - в `_`,
/// края без `_`, не длиннее 40 знаков. Пустое остаётся пустым.
pub fn san(zfield: &str) -> String {
    let mut s = String::new();
    let mut gap = false;
    for c in zfield.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            if gap {
                s.push('_');
                gap = false;
            }
            s.push(c);
        } else {
            gap = true;
        }
    }
    if gap {
        s.push('_');
    }
    s.trim_matches('_').chars().take(40).collect()
}

/// Цифры для отчёта, как `result.stats` модуля.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub naive: f64,
    pub decl: f64,
    pub best_cell: f64,
    pub sizes: Option<Vec<f64>>,
    pub means: Option<Vec<f64>>,
    pub weights: Vec<f64>,
}

/// Расчёт: выходы по ключам в формате прогонов модуля и цифры отчёта.
pub fn run(v: &Values<'_>, report: &mut Report) -> Result<(J, Stats), ToolError> {
    let zfield = v.text("ZFIELD");
    let p = read_points(
        v.items("INPUT"),
        &zfield,
        v.layer("INPUT"),
        report,
        Outliers::default(),
        None,
    )?;
    let n = p.xs.len();
    if n < 2 {
        return Err(ToolError::new("Слишком мало точек.", args![]));
    }
    let aspect = v.number("ASPECT");
    let noff = v.integer("NOFF");
    let st = if v.integer("MODE") == 1 {
        let mut cell = v.number("CELL_SIZE");
        if cell.is_nan() || cell <= 0.0 {
            let (lo, hi) = declus::suggest_range(&p.xs, &p.ys);
            cell = (lo * hi).sqrt();
            report.info("Размер ячейки не задан - взят %.4g.", args![cell]);
        }
        let (weights, decl) = declus::cell_declus(&p.xs, &p.ys, &p.vs, cell, cell * aspect, noff);
        Stats {
            naive: crate::numpy::mean(&p.vs),
            decl,
            best_cell: cell,
            sizes: None,
            means: None,
            weights,
        }
    } else {
        let (lo, hi) = declus::suggest_range(&p.xs, &p.ys);
        let s = declus::declus_sweep(
            &p.xs,
            &p.ys,
            &p.vs,
            lo,
            hi,
            v.integer("NCELL"),
            noff,
            aspect,
            v.integer("OBJECTIVE") == 1,
        );
        Stats {
            naive: s.naive_mean,
            decl: s.decl_mean,
            best_cell: s.best_cell,
            sizes: Some(s.sizes),
            means: Some(s.means),
            weights: s.weights,
        }
    };
    let pct = if st.naive != 0.0 {
        100.0 * (st.decl - st.naive) / st.naive
    } else {
        0.0
    };
    report.info("== Декластеризация ==", args![]);
    report.info("Точек: %d, размер ячейки: %.4g", args![n, st.best_cell]);
    report.info("Среднее без весов:          %.4g", args![st.naive]);
    report.info(
        "Декластеризованное среднее: %.4g (%+.2f%%)",
        args![st.decl, pct],
    );
    report.info(
        "Это среднее ставьте в «Среднее» простого кригинга, а поле wt - в поле весов SGS.",
        args![],
    );
    // у поля значения подписи нет, модуль его не округляет,
    // вес - по подписи «Вес декластеризации»
    let vname = value_field(&zfield);
    let names = [vname.as_str(), "wt"];
    let plan = plan_for(&names, FIELD_LABELS, &[], &v.input_fields());
    let features: Vec<J> = (0..n)
        .map(|i| {
            let (values, raw) = row(&names, vec![json!(p.vs[i]), json!(st.weights[i])], &plan);
            json!({
                "geometry": {"xyz": [p.xs[i], p.ys[i], null]},
                "values": values,
                "raw": raw,
            })
        })
        .collect();
    let out = json!({"OUTPUT": {
        "geometry": "point",
        "fields": [
            {"name": vname, "type": "double"},
            {"name": "wt", "type": "double"},
        ],
        "features": features,
        // цифры отчёта, как stats прогона модуля; слой их не берёт
        "stats": {
            "naive": st.naive,
            "decl": st.decl,
            "best_cell": st.best_cell,
            "sizes": st.sizes,
            "means": st.means,
        },
    }});
    Ok((out, st))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_field_like_module() {
        assert_eq!(value_field("K2O, %"), "K2O");
        assert_eq!(value_field("кровля пласта"), "кровля_пласта");
        assert_eq!(value_field("  "), "z");
        assert_eq!(value_field("a--b"), "a_b");
        assert_eq!(value_field(&"x".repeat(50)).len(), 40);
    }
}
