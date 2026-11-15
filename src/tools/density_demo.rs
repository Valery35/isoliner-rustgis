// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 3.08 Пример для плотности (демо), порт `grid_isolines/tools/density_demo.py`.
//!
//! Точки, линии и полигоны учебного набора с круглой суммарной массой
//! 1000 в охвате слоя. Вспомогательного растра для дазиметрии нет:
//! растров у расширений RustGIS нет.

use serde_json::{Value as J, json};

use crate::args;
use crate::density::demo_dataset;
use crate::field_format::{plan_for, row};
use crate::npy_rng::Rng;
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::tools::geophys_profiles::bbox;

/// Подписи полей выходов (`FIELD_LABELS` класса).
pub const FIELD_LABELS: &[(&str, &str)] = &[
    ("mass", "Масса"),
    ("prec", "Размытие σ, м"),
    ("from_m", "Начало участка, м"),
    ("to_m", "Конец участка, м"),
    ("dasy", "Дазиметрия (1 = да)"),
];

/// Вершины в формате модуля: Z пустая.
pub fn xyz(pts: &[[f64; 2]]) -> J {
    J::Array(pts.iter().map(|p| json!([p[0], p[1], null])).collect())
}

/// Выход с полями: имена и типы.
pub fn layer(geometry: &str, fields: &[(&str, &str)], features: Vec<J>) -> J {
    let f: Vec<J> = fields
        .iter()
        .map(|(n, t)| json!({"name": n, "type": t}))
        .collect();
    json!({"geometry": geometry, "fields": f, "features": features})
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let (x0, y0, x1, y1) = bbox(v.items("EXTENT"))
        .filter(|e| e.2 > e.0 && e.3 > e.1)
        .ok_or_else(|| ToolError::new("Не задан охват.", args![]))?;
    let mut rng = Rng::from_module_seed(v.integer("SEED"));
    let ds = demo_dataset(x0, y0, x1, y1, &mut rng);
    let skip = v.input_fields();

    let pf = [("mass", "double"), ("prec", "double")];
    let names: Vec<&str> = pf.iter().map(|f| f.0).collect();
    let plan = plan_for(&names, FIELD_LABELS, &[], &skip);
    let points = ds
        .points
        .iter()
        .map(|(x, y, m, s)| {
            let (values, raw) = row(&names, vec![json!(m), json!(s)], &plan);
            json!({"geometry": {"xyz": [x, y, null]}, "values": values, "raw": raw})
        })
        .collect();

    let lf = [
        ("mass", "double"),
        ("prec", "double"),
        ("from_m", "double"),
        ("to_m", "double"),
    ];
    let names: Vec<&str> = lf.iter().map(|f| f.0).collect();
    let plan = plan_for(&names, FIELD_LABELS, &[], &skip);
    let lines = ds
        .lines
        .iter()
        .map(|l| {
            let raw = vec![json!(l.mass), json!(l.half), json!(l.from_m), json!(l.to_m)];
            let (values, raw) = row(&names, raw, &plan);
            json!({"geometry": {"points": xyz(&l.verts)}, "values": values, "raw": raw})
        })
        .collect();

    let gf = [("mass", "double"), ("dasy", "int")];
    let names: Vec<&str> = gf.iter().map(|f| f.0).collect();
    let plan = plan_for(&names, FIELD_LABELS, &[], &skip);
    let polys = ds
        .polygons
        .iter()
        .map(|p| {
            let rings: Vec<J> = p.rings.iter().map(|r| xyz(r)).collect();
            let raw = vec![json!(p.mass), json!(i64::from(p.dasy))];
            let (values, raw) = row(&names, raw, &plan);
            json!({"geometry": {"rings": rings}, "values": values, "raw": raw})
        })
        .collect();

    report.info(
        "Демо создано. Масса: точки 500, линии 200, полигоны 300, итого 1000. \
         Прогоните 3.07 на каждом слое - интеграл плотности должен совпасть.",
        args![],
    );
    Ok(json!({
        "OUT_POINTS": layer("point", &pf, points),
        "OUT_LINES": layer("line", &lf, lines),
        "OUT_POLYGONS": layer("polygon", &gf, polys),
    }))
}
