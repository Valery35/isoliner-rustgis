// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 7.04 Размерность Минковского (векторы), порт `grid_isolines/tools/minkowski.py`.
//!
//! У каждого объекта меряются все части, у полигона все кольца. Выход -
//! те же объекты с полями D_mink и D_r2. Размерность всего слоя и ряд
//! «размер ячейки - количество ячеек» идут в журнал и в `stats`.

use serde_json::{Value as J, json};

use crate::args;
use crate::field_format::{Spec, plan_for, round_value, row};
use crate::fractal::minkowski_dimension;
use crate::report::{Report, ToolError};
use crate::tools::Values;

/// Подписи полей выхода (`FIELD_LABELS` класса).
pub const FIELD_LABELS: &[(&str, &str)] = &[
    ("D_mink", "Размерность Минковского"),
    ("D_r2", "Качество аппроксимации R²"),
];

/// Части объекта не короче двух вершин, только X и Y.
fn parts(it: &J) -> Vec<Vec<[f64; 2]>> {
    it["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_array)
        .map(|p| {
            p.iter()
                .map(|q| {
                    [
                        q[0].as_f64().unwrap_or(f64::NAN),
                        q[1].as_f64().unwrap_or(f64::NAN),
                    ]
                })
                .collect::<Vec<_>>()
        })
        .filter(|p| p.len() >= 2)
        .collect()
}

fn r4(x: f64) -> J {
    if x.is_nan() {
        J::Null
    } else {
        json!(round_value(x, Spec::Decimals(4)))
    }
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let n_sizes = v.integer("N_SIZES").max(0) as usize;
    let offsets = v.integer("OFFSETS").max(0) as usize;
    let densify = v.number("DENSIFY");
    let names = ["D_mink", "D_r2"];
    let plan = plan_for(&names, FIELD_LABELS, &[], &v.input_fields());
    let mut all = Vec::new();
    let mut features = Vec::new();
    for it in v.items("FEATURES") {
        let ps = parts(it);
        let (mut d, mut r2) = (f64::NAN, f64::NAN);
        if !ps.is_empty() {
            let m = minkowski_dimension(&ps, n_sizes, offsets, densify, 0);
            d = m.d;
            r2 = m.r2;
        }
        all.extend(ps);
        let id = it["id"].clone();
        let (values, raw) = row(&names, vec![r4(d), r4(r2)], &plan);
        // поля объекта источника идут в слой, одноимённые со своими - нет
        let named: serde_json::Map<String, J> = it["attrs"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(k, _)| !names.contains(&k.as_str()))
            .map(|(k, x)| (k.clone(), x.clone()))
            .collect();
        features.push(json!({
            "geometry": {"source": id},
            "source": id,
            "values": values,
            "raw": raw,
            "named": named,
        }));
    }
    let (mut d_layer, mut r2_layer) = (f64::NAN, f64::NAN);
    let (mut sizes, mut counts) = (Vec::new(), Vec::new());
    if !all.is_empty() {
        let m = minkowski_dimension(&all, n_sizes, offsets, densify, 0);
        for (s, c) in m.sizes.iter().zip(&m.counts) {
            report.info("  %.6g -> N = %d", args![*s, *c]);
        }
        (d_layer, r2_layer, sizes, counts) = (m.d, m.r2, m.sizes, m.counts);
    }
    report.info(
        "Размерность Минковского слоя: D = %.3f (R² = %.3f).",
        args![d_layer, r2_layer],
    );
    if !r2_layer.is_nan() && r2_layer < 0.85 {
        report.warn(
            "R² ниже 0.85: степенной закон не выдержан, оценке доверять нельзя \
             (уменьшите K или проверьте данные).",
            args![],
        );
    }
    Ok(json!({"OUTPUT": {
        "geometry": "source",
        "keep_source": "FEATURES",
        "fields": [{"name": "D_mink", "type": "double"}, {"name": "D_r2", "type": "double"}],
        "features": features,
        "stats": {"D_layer": d_layer, "R2_layer": r2_layer, "sizes": sizes, "counts": counts},
    }}))
}
