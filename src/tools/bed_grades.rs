// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 4.13 Содержания по пласту в устьях, порт `grid_isolines/tools/bed_grades.py`.
//!
//! COLLAR - точки устьев, INTERVAL и CHEM - таблицы. Пробы приводятся к
//! интервалам заданного пласта по длине перекрытия и ложатся одним
//! числом в точку устья. Охват считается по каждому компоненту отдельно.

use std::collections::HashMap;

use serde_json::{Value as J, json};

use crate::args;
use crate::drillhole::{self, Composite, Sample, clean_span, parse_id, parse_num, py_str};
use crate::field_format::{Spec, plan_for, round_value, row};
use crate::numpy::py_sum;
use crate::report::{Report, ToolError};
use crate::tools::Values;

/// Подписи полей выхода (`FIELD_LABELS` класса).
pub const FIELD_LABELS: &[(&str, &str)] = &[
    ("hole_id", "Скважина"),
    ("bed", "Пласт"),
    ("len_m", "Мощность по стволу, м"),
    ("cover", "Охват пробами, доля"),
    ("n_samp", "Проб"),
    ("cov_*", "Охват по %s, доля"),
];

/// Имена полей слоя: из сведений `@КЛЮЧ`, без них - по атрибутам строк.
fn names(v: &Values<'_>, key: &str) -> Vec<String> {
    if let Some(f) = v.layer(key)["fields"].as_array() {
        return f
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    let mut out: Vec<String> = Vec::new();
    for it in v.items(key) {
        for k in it["attrs"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
        {
            if !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

fn val<'a>(row: &'a J, field: &str) -> &'a J {
    &row["attrs"][field]
}

/// Округление `round(x, n)` Python.
fn r(x: f64, n: i32) -> f64 {
    round_value(x, Spec::Decimals(n))
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let has = |k: &str| v.values[k].is_array();
    if !(has("COLLAR") && has("INTERVAL") && has("CHEM")) {
        return Err(ToolError::new(
            "Нужны три таблицы: устья, интервалы и пробы.",
            args![],
        ));
    }
    let bed = v.text("BED").trim().to_string();
    if bed.is_empty() {
        return Err(ToolError::new(
            "Не задан код пласта. Он берётся из поля кода в таблице интервалов, например КрII.",
            args![],
        ));
    }
    let comps: Vec<String> = match v.get("COMPONENTS") {
        J::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        J::String(s) if !s.trim().is_empty() => vec![s.trim().to_string()],
        _ => Vec::new(),
    };
    if comps.is_empty() {
        return Err(ToolError::new(
            "Не выбрано ни одного поля содержаний. Набор компонентов у разных \
             предприятий разный, поэтому задаётся полями самой таблицы проб.",
            args![],
        ));
    }
    let min_cover = {
        let m = v.number("MIN_COVER");
        if m.is_nan() { 0.0 } else { m }
    };
    let (cn, inn, hn) = (names(v, "COLLAR"), names(v, "INTERVAL"), names(v, "CHEM"));
    let field =
        |n: &[String], key: &str, wanted: &str| drillhole::resolve_field(n, &v.text(key), wanted);
    let found = [
        ("устья: идентификатор", field(&cn, "CID", "hole_id")),
        ("интервалы: идентификатор", field(&inn, "IID", "hole_id")),
        ("интервалы: кровля", field(&inn, "IFROM", "from")),
        ("интервалы: подошва", field(&inn, "ITO", "to")),
        ("интервалы: код", field(&inn, "ICODE", "code")),
        ("пробы: идентификатор", field(&hn, "HID", "hole_id")),
        ("пробы: кровля", field(&hn, "HFROM", "from")),
        ("пробы: подошва", field(&hn, "HTO", "to")),
    ];
    let missing: Vec<&str> = found
        .iter()
        .filter(|(_, f)| f.is_none())
        .map(|(n, _)| *n)
        .collect();
    if !missing.is_empty() {
        return Err(ToolError::new(
            "Не найдены поля: %s. Укажите их явно.",
            args![missing.join(", ")],
        ));
    }
    let f: Vec<String> = found
        .iter()
        .map(|(_, x)| x.clone().unwrap_or_default())
        .collect();
    let (cid, iid, ifrom, ito, icode, hid, hfrom, hto) =
        (&f[0], &f[1], &f[2], &f[3], &f[4], &f[5], &f[6], &f[7]);

    // целевые интервалы заданного кода по скважинам, в порядке появления
    let mut targets: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    let mut n_int = 0usize;
    for row in v.items("INTERVAL") {
        let code = val(row, icode);
        if code.is_null() || py_str(code).trim() != bed {
            continue;
        }
        let (Some(hole), Some(span)) = (
            parse_id(val(row, iid)),
            clean_span(val(row, ifrom), val(row, ito)),
        ) else {
            continue;
        };
        targets.entry(hole).or_default().push(span);
        n_int += 1;
    }
    if targets.is_empty() {
        return Err(ToolError::new(
            "Пласт «%s» не встретился ни в одной скважине. Проверьте код и поле кода.",
            args![bed],
        ));
    }

    // пробы по скважинам; пустые значения не подставляются нулём
    let mut samples: HashMap<String, Vec<Sample>> = HashMap::new();
    let mut n_samp = 0usize;
    for row in v.items("CHEM") {
        let Some(hole) = parse_id(val(row, hid)) else {
            continue;
        };
        if !targets.contains_key(&hole) {
            continue;
        }
        let values = comps
            .iter()
            .filter_map(|c| parse_num(val(row, c)).map(|x| (c.clone(), x)))
            .collect();
        samples.entry(hole).or_default().push(Sample {
            from: val(row, hfrom).clone(),
            to: val(row, hto).clone(),
            values,
        });
        n_samp += 1;
    }
    report.info(
        "Химия: проб %d по %d скважинам, компоненты %s.",
        args![n_samp, samples.len(), comps.join(", ")],
    );

    let mut names_out: Vec<String> = ["hole_id", "bed", "len_m", "cover", "n_samp"]
        .map(String::from)
        .to_vec();
    let mut fields = vec![
        json!({"name": "hole_id", "type": "string"}),
        json!({"name": "bed", "type": "string"}),
        json!({"name": "len_m", "type": "double"}),
        json!({"name": "cover", "type": "double"}),
        json!({"name": "n_samp", "type": "int"}),
    ];
    for c in &comps {
        let cov: String = format!("cov_{c}").chars().take(60).collect();
        fields.push(json!({"name": c, "type": "double"}));
        fields.push(json!({"name": cov, "type": "double"}));
        names_out.push(c.clone());
        names_out.push(cov);
    }
    let nrefs: Vec<&str> = names_out.iter().map(String::as_str).collect();
    let plan = plan_for(&nrefs, FIELD_LABELS, &[], &v.input_fields());

    let (mut n_out, mut n_empty, mut n_low, mut n_over) = (0usize, 0usize, 0usize, 0usize);
    let mut cover_sum = 0.0;
    let mut features = Vec::new();
    for col in v.items("COLLAR") {
        let Some(hole) = parse_id(val(col, cid)) else {
            continue;
        };
        let Some(tg) = targets.get(&hole) else {
            continue;
        };
        if col["xyz"].is_null() {
            continue;
        }
        let rows: Vec<Composite> =
            drillhole::composite(samples.get(&hole).map_or(&[], Vec::as_slice), tg, &comps);
        let total = py_sum(rows.iter().map(|r| r.length));
        if total <= 0.0 {
            continue;
        }
        let mut attrs = Vec::new();
        let cover_any = py_sum(rows.iter().map(|r| r.length * r.cover_any)) / total;
        let hits: usize = rows.iter().map(|r| r.n_samples).sum();
        if rows.iter().any(|r| r.overlap > 1e-9) {
            n_over += 1;
        }
        if hits == 0 {
            n_empty += 1;
        } else if cover_any < min_cover {
            n_low += 1;
        }
        cover_sum += cover_any;
        attrs.push(json!(hole));
        attrs.push(json!(bed));
        attrs.push(json!(r(total, 3)));
        attrs.push(json!(r(cover_any, 4)));
        attrs.push(json!(hits));
        for k in 0..comps.len() {
            let wsum = py_sum(rows.iter().map(|r| r.length * r.cover[k]));
            let value = (wsum > 0.0).then(|| {
                py_sum(
                    rows.iter()
                        .filter_map(|r| r.values[k].map(|x| r.length * r.cover[k] * x)),
                ) / wsum
            });
            attrs.push(value.map_or(J::Null, |x| json!(r(x, 6))));
            attrs.push(json!(r(wsum / total, 4)));
        }
        let xyz = &col["xyz"];
        let (values, raw) = row(&nrefs, attrs, &plan);
        features.push(json!({
            "geometry": {"xyz": [xyz[0], xyz[1], xyz[2]]},
            "values": values,
            "raw": raw,
        }));
        n_out += 1;
    }
    if n_out == 0 {
        return Err(ToolError::new(
            "Ни одна скважина не получила значений. Устья не стыкуются с \
             интервалами по идентификатору.",
            args![],
        ));
    }
    report.info(
        "Химия: приведено к %d интервалам пласта «%s» в %d скважинах, средний охват %.0f процентов.",
        args![n_int, bed, n_out, 100.0 * cover_sum / n_out as f64],
    );
    report.info(
        "Химия: скважин без проб по пласту %d, с охватом ниже порога %d, с перекрытием проб %d.",
        args![n_empty, n_low, n_over],
    );
    if n_low > 0 || n_empty > 0 {
        report.warn(
            "Значения с низким охватом остаются в слое. Выбрасывать ли их, решает \
             пользователь. Отбирайте по полю «cover» перед кригингом.",
            args![],
        );
    }
    Ok(json!({"OUTPUT": {"geometry": "point", "fields": fields, "features": features}}))
}
