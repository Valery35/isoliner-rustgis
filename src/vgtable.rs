// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Таблица моделей вариограмм, порт `grid_isolines/variogram_table.py`
//! (`parse_rows`, `to_variogram`): строка на структуру, негодная строка
//! пропускается с замечанием.

use std::collections::BTreeMap;

use serde_json::Value as J;

use crate::drillhole::py_str;
use crate::krige::{Structure, Variogram};
use crate::pyfmt::Arg;

/// Модель: наггет, структуры, настройки отсева.
#[derive(Debug, Clone)]
pub struct Model {
    pub profile: String,
    pub field: String,
    pub nugget: f64,
    pub val_pct: f64,
    pub val_cap: bool,
    /// (номер структуры, структура).
    pub structs: Vec<(i64, Structure)>,
}

impl Model {
    pub fn variogram(&self) -> Variogram {
        let s: Vec<Structure> = self.structs.iter().map(|(_, s)| *s).collect();
        Variogram::new(self.nugget, &s)
    }
}

/// Мягкий разбор числа (`_num`): запятая как точка, пробелы, пустое.
fn num(v: &J) -> Option<f64> {
    match v {
        J::Null => None,
        J::Bool(b) => Some(f64::from(u8::from(*b))),
        J::Number(n) => n.as_f64(),
        other => {
            let t = py_str(other).trim().replace(['\u{a0}', ' '], "");
            if t.is_empty() {
                return None;
            }
            t.replace(',', ".").parse().ok()
        }
    }
}

fn flag(v: &J) -> bool {
    match v {
        J::Null => false,
        J::Bool(b) => *b,
        other => {
            let t = py_str(other).trim().to_lowercase();
            ["1", "true", "да", "yes", "y", "истина", "t"].contains(&t.as_str())
        }
    }
}

fn model_code(v: &J) -> Option<i64> {
    if v.is_null() {
        return None;
    }
    match py_str(v).trim().to_lowercase().as_str() {
        "сферическая" | "spherical" | "sph" | "1" => Some(1),
        "экспоненциальная" | "exponential" | "exp" | "2" => Some(2),
        "гауссова" | "gaussian" | "gau" | "3" => Some(3),
        "степенная" | "power" | "pow" | "4" => Some(4),
        _ => None,
    }
}

/// `repr()` Python для значения ячейки.
fn py_repr(v: &J) -> String {
    match v {
        J::String(s) => {
            if s.contains('\'') && !s.contains('"') {
                format!("\"{s}\"")
            } else {
                format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
            }
        }
        other => py_str(other),
    }
}

/// Строки таблицы (атрибуты) в модели по имени профиля, в порядке
/// появления, и замечания по строкам.
pub fn parse_rows(rows: &[J]) -> (Vec<Model>, Vec<String>) {
    let mut notes = Vec::new();
    let mut drafts: Vec<Model> = Vec::new();
    let mut seen: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for (k, raw) in rows.iter().enumerate() {
        let n = k + 1;
        let row: BTreeMap<String, &J> = raw
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, v)| (key.trim().to_lowercase(), v))
            .collect();
        let get = |key: &str| row.get(key).copied().unwrap_or(&J::Null);
        let name = match get("profile") {
            J::Null => String::new(),
            v => py_str(v).trim().to_string(),
        };
        let name = if get("profile").as_str() == Some("") {
            String::new()
        } else {
            name
        };
        if name.is_empty() {
            notes.push(format!("строка {n}: пустое имя профиля"));
            continue;
        }
        let Some(code) = model_code(get("model")) else {
            notes.push(format!(
                "строка {n} ({name}): модель не распознана: {}",
                py_repr(get("model"))
            ));
            continue;
        };
        let sill = num(get("sill"));
        let rng = num(get("range"));
        let Some(sill) = sill.filter(|s| *s >= 0.0) else {
            notes.push(format!(
                "строка {n} ({name}): вклад C не число или отрицателен"
            ));
            continue;
        };
        let Some(rng) = rng.filter(|r| *r > 0.0) else {
            notes.push(format!(
                "строка {n} ({name}): радиус корреляции должен быть больше нуля"
            ));
            continue;
        };
        let st = num(get("struct")).unwrap_or(1.0);
        let st = if st == 0.0 { 1 } else { st as i64 };
        let mut anis = num(get("anis")).unwrap_or(1.0);
        if anis <= 0.0 {
            notes.push(format!(
                "строка {n} ({name}): анизотропия должна быть больше нуля, взята единица"
            ));
            anis = 1.0;
        }
        let pos = drafts.iter().position(|m| m.profile == name);
        let m = match pos {
            Some(p) => {
                let m = &drafts[p];
                for (key, got, want) in [
                    ("nugget", num(get("nugget")).unwrap_or(m.nugget), m.nugget),
                    (
                        "val_pct",
                        num(get("val_pct")).unwrap_or(m.val_pct),
                        m.val_pct,
                    ),
                ] {
                    if (got - want).abs() > 1e-9 {
                        notes.push(crate::pyfmt::format(
                            "строка %d (%s): %s отличается от заданного в первой строке \
                             профиля (%.4g против %.4g), взято первое",
                            &[
                                Arg::from(n),
                                Arg::from(name.as_str()),
                                Arg::from(key),
                                Arg::from(got),
                                Arg::from(want),
                            ],
                        ));
                    }
                }
                p
            }
            None => {
                let text = |key: &str| match get(key) {
                    J::Null => String::new(),
                    J::String(s) if s.is_empty() => String::new(),
                    v => py_str(v).trim().to_string(),
                };
                let nz = |v: Option<f64>| v.filter(|x| *x != 0.0).unwrap_or(0.0);
                drafts.push(Model {
                    profile: name.clone(),
                    field: text("field"),
                    nugget: nz(num(get("nugget"))),
                    val_pct: nz(num(get("val_pct"))),
                    val_cap: flag(get("val_cap")),
                    structs: Vec::new(),
                });
                drafts.len() - 1
            }
        };
        let s = seen.entry(name.clone()).or_default();
        if s.contains(&st) {
            notes.push(format!(
                "строка {n} ({name}): структура {st} уже была, строка пропущена"
            ));
            continue;
        }
        s.push(st);
        drafts[m].structs.push((
            st,
            Structure {
                it: code,
                cc: sill,
                aa: rng,
                ang: num(get("azimuth")).unwrap_or(0.0),
                anis,
            },
        ));
    }
    let mut out = Vec::new();
    for mut m in drafts {
        if m.structs.is_empty() {
            notes.push(format!("профиль {}: ни одной годной структуры", m.profile));
            continue;
        }
        m.structs.sort_by_key(|(n, _)| *n);
        out.push(m);
    }
    (out, notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rows_to_models() {
        let rows = vec![
            json!({"Profile": "a", "model": "сферическая", "sill": "1,5", "range": 100, "nugget": 0.5}),
            json!({"profile": "a", "struct": 2, "model": 2, "sill": 1, "range": 50, "nugget": 0.7}),
            json!({"profile": "b", "model": "xx", "sill": 1, "range": 1}),
            json!({"profile": "", "model": 1}),
        ];
        let (m, notes) = parse_rows(&rows);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].nugget, 0.5);
        assert_eq!(m[0].structs.len(), 2);
        assert_eq!(m[0].structs[0].1.cc, 1.5);
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(notes[0].contains("nugget отличается"), "{notes:?}");
        assert_eq!(notes[1], "строка 3 (b): модель не распознана: 'xx'");
    }
}
