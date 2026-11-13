// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 2.08 Разделить горизонтали для проверки, порт `grid_isolines/tools/contour_split.py`.
//!
//! Каждая EVERY-я отметка со сдвигом OFFSET уходит в проверку целиком,
//! крайние отметки остаются в построении. Выходы OUTPUT_BUILD и
//! OUTPUT_CHECK - те же горизонтали с полем hold: 0 построение, 1 проверка.

use serde_json::{Value as J, json};

use crate::args;
use crate::points::num;
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::validate::{contour_interval, split_levels};

/// Объект источника с полем hold, поля источника идут в слой.
fn feature(it: &J, hold: i64) -> J {
    let id = it["id"].clone();
    let named: serde_json::Map<String, J> = it["attrs"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.as_str() != "hold")
        .map(|(k, x)| (k.clone(), x.clone()))
        .collect();
    json!({
        "geometry": {"source": id},
        "source": id,
        "values": [hold],
        "raw": [hold],
        "named": named,
    })
}

fn output(features: Vec<J>) -> J {
    json!({
        "geometry": "source",
        "keep_source": "INPUT",
        "fields": [{"name": "hold", "type": "int"}],
        "features": features,
    })
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let field = v.text("FIELD");
    let every = match v.integer("EVERY") {
        0 => 4,
        k => k,
    };
    let offset = v.integer("OFFSET");
    let mut feats: Vec<(&J, f64)> = Vec::new();
    for it in v.items("INPUT") {
        let has_geom = it["parts"].as_array().is_some_and(|ps| {
            ps.iter()
                .any(|p| p.as_array().is_some_and(|a| !a.is_empty()))
        });
        if !has_geom {
            continue;
        }
        let Some(lv) = num(&it["attrs"][field.as_str()]).filter(|x| !x.is_nan()) else {
            continue;
        };
        feats.push((it, lv));
    }
    if feats.is_empty() {
        return Err(ToolError::new(
            "В слое нет горизонталей с числовой отметкой.",
            args![],
        ));
    }
    let levels: Vec<f64> = feats.iter().map(|(_, lv)| *lv).collect();
    let (build_lv, check_lv) = split_levels(&levels, every, offset);
    if check_lv.is_empty() {
        return Err(ToolError::new(
            "Проверочный набор пуст, уровней слишком мало для такого шага.",
            args![],
        ));
    }
    let mut build = Vec::new();
    let mut check = Vec::new();
    for (it, lv) in &feats {
        if check_lv.contains(lv) {
            check.push(feature(it, 1));
        } else {
            build.push(feature(it, 0));
        }
    }
    report.info(
        "Уровней всего %d: в построение %d, в проверку %d. Объектов %d и %d.",
        args![
            build_lv.len() + check_lv.len(),
            build_lv.len(),
            check_lv.len(),
            build.len(),
            check.len()
        ],
    );
    if let Some(iv) = contour_interval(&levels, 1e-6).filter(|x| *x != 0.0) {
        report.info("Сечение рельефа по набору: %.4g.", args![iv]);
    }
    Ok(json!({"OUTPUT_BUILD": output(build), "OUTPUT_CHECK": output(check)}))
}
