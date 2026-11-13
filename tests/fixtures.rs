// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Сверка с прогонами модуля QGIS (`fixtures/tool_runs.json`, пишет
//! `tools/make_fixtures.py` модуля). Каждый прогон инструмента
//! расширения считается заново по его `values` и `input`. Поля выхода,
//! точки и значения сверяются в пределах `tolerance` прогона, строки
//! журнала и текст отказа - слово в слово.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use isoliner_rustgis::product::TOOLS_JSON;
use isoliner_rustgis::pyfmt::Arg;
use isoliner_rustgis::report::Report;
use isoliner_rustgis::tools;
use serde_json::{Value as J, json};

/// Допуск сверки по умолчанию: расчёт тот же, порядок сложений тот же.
const TOL: f64 = 1e-9;

fn runs() -> Option<J> {
    let path = format!("{}/fixtures/tool_runs.json", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(path).ok()?;
    Some(serde_json::from_slice(&bytes).unwrap())
}

/// Допуск поля: `tolerance` прогона по имени поля, иначе общий
/// (`values_rel`), иначе [`TOL`]. Относительный к величине больше 1.
fn tol_for(tol: &J, field: &str) -> f64 {
    [field, "values_rel", "values", "abs"]
        .iter()
        .find_map(|k| tol[*k].as_f64())
        .unwrap_or(TOL)
}

fn close(a: &J, b: &J, tol: f64) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() <= tol * y.abs().max(1.0),
        // NaN модуля в JSON - null
        _ => a.is_null() && b.is_null() || a == b,
    }
}

/// Выход прогона модуля по ключам: один выход модуль может писать без
/// ключа, сразу полями.
fn outputs(r: &J) -> Vec<(String, J)> {
    let o = &r["output"];
    if o.get("fields").is_some() {
        return vec![("OUTPUT".into(), o.clone())];
    }
    o.as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn field_names(o: &J) -> Vec<String> {
    o["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            f["name"]
                .as_str()
                .or_else(|| f[0].as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn check_run(cat: &rustgis_kit::Catalog, r: &J, n: usize) -> bool {
    let name = r["tool"].as_str().unwrap_or("");
    let Some(tool) = cat.tool(name) else {
        return false;
    };
    // values и input прогона - одно пространство ключей
    let mut values = r["values"].clone();
    for (k, v) in r["input"].as_object().into_iter().flatten() {
        values[k] = v.clone();
    }
    let mut report = Report::default();
    // модуль пишет образцы на русском: данные со словами (подпись пикета
    // 1.11) и журнал - по-русски
    let Some(got) = tools::run(tool, &values, rustgis_kit::Lang::Ru, &mut report) else {
        return false;
    };
    let tag = format!("{name} прогон {n}");
    let lines: Vec<J> = report
        .lines
        .iter()
        .map(|l| json!([l.level, l.text()]))
        .collect();
    match (got, r.get("error")) {
        (Err(e), Some(want)) => {
            // отказ модуля - шаблон с аргументами, либо готовый текст
            match want.as_str() {
                Some(text) => assert_eq!(e.text(), text, "{tag}: отказ"),
                None => {
                    assert_eq!(
                        e.template,
                        want["template"].as_str().unwrap_or(""),
                        "{tag}: отказ"
                    );
                    let args: Vec<J> = e
                        .args
                        .iter()
                        .map(|a| match a {
                            Arg::Int(i) => json!(i),
                            Arg::Float(f) => json!(f),
                            Arg::Text(t) => json!(t),
                        })
                        .collect();
                    assert_eq!(json!(args), want["args"], "{tag}: аргументы отказа");
                }
            }
        }
        (Err(e), None) => panic!("{tag}: отказ «{}», у модуля расчёт", e.text()),
        (Ok(_), Some(want)) => panic!("{tag}: расчёт, у модуля отказ {want}"),
        (Ok(out), None) => {
            let tol = &r["tolerance"];
            // цифры отчёта прогона (stats) - у выхода OUTPUT расширения
            if let Some(want) = r["stats"].as_object() {
                let got = &out["OUTPUT"]["stats"];
                for (k, w) in want {
                    let g = &got[k];
                    match (w.as_array(), g.as_array()) {
                        (Some(wa), Some(ga)) => {
                            assert_eq!(ga.len(), wa.len(), "{tag}: stats.{k}");
                            for (i, (a, b)) in ga.iter().zip(wa).enumerate() {
                                assert!(
                                    close(a, b, tol_for(tol, k)),
                                    "{tag}: stats.{k}[{i}] {a} против {b}"
                                );
                            }
                        }
                        _ => assert!(
                            close(g, w, tol_for(tol, k)),
                            "{tag}: stats.{k} {g} против {w}"
                        ),
                    }
                }
            }
            // лишних выходов, которых модуль не дал, нет
            let keys: Vec<String> = outputs(r).into_iter().map(|(k, _)| k).collect();
            for k in out.as_object().into_iter().flatten().map(|(k, _)| k) {
                assert!(keys.contains(k), "{tag}: лишний выход {k}");
            }
            for (key, want) in outputs(r) {
                let o = &out[&key];
                assert!(o.is_object(), "{tag}: нет выхода {key}");
                let names = field_names(&want);
                assert_eq!(field_names(o), names, "{tag}: поля {key}");
                let (wf, gf) = (
                    want["features"].as_array().cloned().unwrap_or_default(),
                    o["features"].as_array().cloned().unwrap_or_default(),
                );
                assert_eq!(gf.len(), wf.len(), "{tag}: объектов в {key}");
                for (i, (g, w)) in gf.iter().zip(&wf).enumerate() {
                    // объект источника - тот же, что у модуля
                    if !w["source"].is_null() {
                        assert_eq!(g["source"], w["source"], "{tag}: {key}[{i}].source");
                    }
                    // values - после округления полей, raw - точные числа
                    for part in ["values", "raw"] {
                        if w[part].is_null() {
                            continue;
                        }
                        for (j, field) in names.iter().enumerate() {
                            // поля, которые модуль просит не сверять (дата подбора)
                            if tol["skip_fields"]
                                .as_array()
                                .is_some_and(|s| s.iter().any(|x| x == field.as_str()))
                            {
                                continue;
                            }
                            let (a, b) = (&g[part][j], &w[part][j]);
                            assert!(
                                close(a, b, tol_for(tol, field)),
                                "{tag}: {key}[{i}].{part}.{field} {a} против {b}"
                            );
                        }
                    }
                    let xyz = w["xyz"]
                        .as_array()
                        .or_else(|| w["geometry"]["xyz"].as_array());
                    if let Some(xyz) = xyz {
                        for (c, b) in xyz.iter().enumerate() {
                            let a = &g["geometry"]["xyz"][c];
                            assert!(
                                close(a, b, tol_for(tol, "xyz")),
                                "{tag}: {key}[{i}].xyz[{c}] {a} против {b}"
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(want) = r["report"].as_array() {
        assert_eq!(&lines, want, "{tag}: журнал");
    }
    true
}

#[test]
fn runs_of_the_module() {
    let cat = rustgis_kit::Catalog::parse(TOOLS_JSON).unwrap();
    let provisional = serde_json::from_str::<J>(TOOLS_JSON).unwrap()["note"].is_string();
    let Some(runs) = runs() else {
        // до выгрузки модуля описание временное, после неё прогоны обязательны
        assert!(
            provisional,
            "tools.json выгружен модулем, а tool_runs.json нет"
        );
        eprintln!("fixtures/tool_runs.json нет: сверка ждёт выгрузки модуля");
        return;
    };
    let mut checked = std::collections::BTreeMap::<String, usize>::new();
    for (n, r) in runs["runs"].as_array().into_iter().flatten().enumerate() {
        if check_run(&cat, r, n) {
            *checked
                .entry(r["tool"].as_str().unwrap_or("").into())
                .or_default() += 1;
        }
    }
    // у каждого инструмента расширения есть хотя бы один прогон
    for name in tools::OFFERED {
        assert!(
            checked.contains_key(*name),
            "{name}: нет прогонов в tool_runs.json"
        );
    }
    eprintln!("сверено прогонов: {checked:?}");
}
