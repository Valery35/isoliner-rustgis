// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 7.03 Размерность линий и границ, порт `grid_isolines/tools/line_dimension.py`.
//!
//! Поле LINES - объекты: части линии или внешние кольца полигона,
//! меряется самая длинная часть по числу вершин. Выход - те же объекты с
//! полями D и steps. Слои RustGIS приходят в градусах, крейт переводит их
//! в метры зоны UTM до расчёта.

use serde_json::{Value as J, json};

use crate::args;
use crate::field_format::{Spec, plan_for, round_value, row};
use crate::fractal::divider_dimension;
use crate::report::{Report, ToolError};
use crate::tools::Values;

/// Подписи полей выхода (`FIELD_LABELS` класса).
pub const FIELD_LABELS: &[(&str, &str)] = &[("D", "Размерность D"), ("steps", "Шагов циркуля")];

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let names = ["D", "steps"];
    let plan = plan_for(&names, FIELD_LABELS, &[], &v.input_fields());
    let (mut dsum, mut dcnt) = (0.0, 0usize);
    let mut features = Vec::new();
    for it in v.items("LINES") {
        let parts: Vec<Vec<[f64; 2]>> = it["parts"]
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
            .filter(|p| !p.is_empty())
            .collect();
        // самая длинная часть по числу вершин, при равенстве первая
        let best = parts.iter().fold(None::<&Vec<[f64; 2]>>, |b, p| match b {
            Some(q) if q.len() >= p.len() => Some(q),
            _ => Some(p),
        });
        let (mut d, mut steps) = (f64::NAN, 0usize);
        if let Some(pts) = best.filter(|p| p.len() >= 3) {
            let (dd, ss) = divider_dimension(pts, 6);
            d = dd;
            steps = ss.last().copied().unwrap_or(0);
        }
        let id = it["id"].clone();
        let raw = vec![
            if d.is_nan() {
                J::Null
            } else {
                json!(round_value(d, Spec::Decimals(4)))
            },
            json!(steps),
        ];
        let (values, raw) = row(&names, raw, &plan);
        // поля объекта источника идут в слой, как у модуля (keep_source);
        // одноимённое со своим полем не берётся, иначе затрёт расчёт
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
        if !d.is_nan() {
            dsum += d;
            dcnt += 1;
        }
    }
    if dcnt > 0 {
        report.info(
            "Среднее D по %d линиям: %.3f.",
            args![dcnt, dsum / dcnt as f64],
        );
    }
    Ok(json!({"OUTPUT": {
        "geometry": "source",
        "keep_source": "LINES",
        "fields": [{"name": "D", "type": "double"}, {"name": "steps", "type": "int"}],
        "features": features,
    }}))
}
