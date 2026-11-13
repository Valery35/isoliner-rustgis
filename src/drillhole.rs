// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Модель данных бурения, часть `grid_isolines/drillhole_core.py` модуля:
//! терпимый поиск полей, разбор чисел и идентификаторов, композит проб по
//! целевым интервалам с весом по длине перекрытия.

use std::collections::BTreeMap;

use serde_json::Value as J;

use crate::numpy::py_sum;

/// Синонимы имён полей: ожидаемое имя первым, дальше частые варианты
/// выгрузок горных пакетов (`FIELD_SYNONYMS`).
fn synonyms(wanted: &str) -> Vec<&str> {
    match wanted {
        "hole_id" => vec!["hole_id", "holeid", "hole", "bhid", "dhid", "well"],
        "z" => vec!["z", "elev", "elevation", "rl", "collar_z"],
        "eoh" => vec!["eoh", "depth", "max_depth", "td", "total_depth"],
        "number" => vec!["number", "name", "label"],
        "from" => vec!["from", "from_", "depth_from", "from_m"],
        "to" => vec!["to", "to_", "depth_to", "to_m"],
        "code" => vec!["code", "litho", "lith", "geol", "class", "seam"],
        other => vec![other],
    }
}

/// Нижний регистр как `str.lower()` Python.
fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Поле из `names` под ожидаемое имя: точное без учёта регистра, затем
/// синонимы по порядку (`find_field`).
pub fn find_field(names: &[String], wanted: &str) -> Option<String> {
    // как словарь Python: при одинаковом нижнем регистре побеждает последнее
    let mut map: BTreeMap<String, &String> = BTreeMap::new();
    for n in names {
        map.insert(lower(n), n);
    }
    synonyms(wanted)
        .iter()
        .find_map(|c| map.get(&lower(c.trim())).map(|n| (*n).clone()))
}

/// Выбор пользователя, если такое поле есть, иначе автопоиск
/// (`resolve_field`).
pub fn resolve_field(names: &[String], chosen: &str, wanted: &str) -> Option<String> {
    if !chosen.is_empty() {
        let want = lower(chosen.trim());
        if let Some(n) = names.iter().rev().find(|n| lower(n) == want) {
            return Some(n.clone());
        }
    }
    find_field(names, wanted)
}

/// Число из атрибута или `None`: строки с запятой и пробелами
/// принимаются, пустое, булево, нечисловое и не конечное - `None`.
pub fn parse_num(v: &J) -> Option<f64> {
    let f = match v {
        J::Number(n) => n.as_f64()?,
        J::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            s.replace(',', ".").replace(' ', "").parse().ok()?
        }
        _ => return None,
    };
    f.is_finite().then_some(f)
}

/// `str()` Python для значения атрибута.
pub fn py_str(v: &J) -> String {
    match v {
        J::Null => "None".into(),
        J::Bool(b) => if *b { "True" } else { "False" }.into(),
        J::String(s) => s.clone(),
        J::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(u)) => u.to_string(),
            _ => crate::pyfmt::Arg::Float(n.as_f64().unwrap_or(f64::NAN)).text(),
        },
        other => other.to_string(),
    }
}

/// Идентификатор скважины: непустая строка без крайних пробелов, целое
/// дробное без хвоста «.0» (`parse_id`).
pub fn parse_id(v: &J) -> Option<String> {
    if v.is_null() {
        return None;
    }
    if let Some(f) = v.as_f64()
        && v.is_f64()
        && f.is_finite()
        && f.fract() == 0.0
    {
        return Some(format!("{f:.0}"));
    }
    let s = py_str(v).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Пара глубин сверху вниз или `None` (`_clean_span`).
pub fn clean_span(frm: &J, to: &J) -> Option<(f64, f64)> {
    let (a, b) = (parse_num(frm)?, parse_num(to)?);
    Some(if a > b { (b, a) } else { (a, b) })
}

/// Длина объединения отрезков (`_union_length`).
fn union_length(spans: &[(f64, f64)]) -> f64 {
    if spans.is_empty() {
        return 0.0;
    }
    let mut s = spans.to_vec();
    s.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut total = 0.0;
    // начало - первый по порядку входа, как в модуле: spans[0]
    let (mut ca, mut cb) = spans[0];
    for &(a, b) in &s[1..] {
        if a > cb {
            total += cb - ca;
            ca = a;
            cb = b;
        } else if b > cb {
            cb = b;
        }
    }
    total + (cb - ca)
}

/// Проба: кровля, подошва и значения компонентов (только числовые).
pub struct Sample {
    pub from: J,
    pub to: J,
    pub values: Vec<(String, f64)>,
}

/// Итог композита по целевому интервалу.
#[derive(Debug, Clone)]
pub struct Composite {
    pub length: f64,
    /// Среднее по компоненту, `None` - данных нет.
    pub values: Vec<Option<f64>>,
    /// Доля интервала, обеспеченная данными, по компоненту.
    pub cover: Vec<f64>,
    pub cover_any: f64,
    pub n_samples: usize,
    pub overlap: f64,
}

/// Средневзвешенные содержания проб по целевым интервалам (`composite`).
/// Вес пробы - длина её перекрытия с интервалом.
pub fn composite(samples: &[Sample], targets: &[(f64, f64)], comps: &[String]) -> Vec<Composite> {
    /// Проба с глубинами по порядку и значениями.
    type Span = (f64, f64, Vec<(String, f64)>);
    let mut prepared: Vec<Span> = samples
        .iter()
        .filter_map(|s| {
            let (a, b) = clean_span(&s.from, &s.to)?;
            Some((a, b, s.values.clone()))
        })
        .collect();
    prepared.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
    targets
        .iter()
        .map(|&(t0, t1)| {
            let length = t1 - t0;
            let mut wsum = vec![0.0; comps.len()];
            let mut vsum = vec![0.0; comps.len()];
            let (mut hits, mut spans, mut w_all) = (0usize, Vec::new(), 0.0);
            for (s0, s1, vals) in &prepared {
                if *s1 <= t0 {
                    continue;
                }
                if *s0 >= t1 {
                    break;
                }
                let w = s1.min(t1) - s0.max(t0);
                if w <= 0.0 {
                    continue;
                }
                hits += 1;
                spans.push((s0.max(t0), s1.min(t1)));
                w_all += w;
                for (k, c) in comps.iter().enumerate() {
                    if let Some((_, v)) = vals.iter().find(|(n, _)| n == c) {
                        wsum[k] += w;
                        vsum[k] += w * v;
                    }
                }
            }
            let cover = wsum
                .iter()
                .map(|w| if length > 0.0 { w / length } else { 0.0 })
                .collect();
            let values = wsum
                .iter()
                .zip(&vsum)
                .map(|(w, v)| (*w > 0.0).then(|| v / w))
                .collect();
            let union = union_length(&spans);
            Composite {
                length,
                values,
                cover,
                cover_any: if length > 0.0 { union / length } else { 0.0 },
                n_samples: hits,
                overlap: (w_all - union).max(0.0),
            }
        })
        .collect()
}

/// Сумма по строкам композита, как `sum()` Python.
pub fn sum_rows(rows: &[Composite], f: impl Fn(&Composite) -> Option<f64>) -> f64 {
    py_sum(rows.iter().filter_map(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fields_and_ids() {
        let names: Vec<String> = ["HoleID", "From_M", "TO", "kcl"].map(String::from).to_vec();
        assert_eq!(find_field(&names, "hole_id").as_deref(), Some("HoleID"));
        assert_eq!(find_field(&names, "from").as_deref(), Some("From_M"));
        assert_eq!(resolve_field(&names, " kCl ", "x").as_deref(), Some("kcl"));
        assert_eq!(resolve_field(&names, "nope", "to").as_deref(), Some("TO"));
        assert_eq!(parse_id(&json!(791.0)).as_deref(), Some("791"));
        assert_eq!(parse_id(&json!(" H1 ")).as_deref(), Some("H1"));
        assert_eq!(parse_id(&json!("  ")), None);
        assert_eq!(parse_num(&json!("1 234,5")), Some(1234.5));
        assert_eq!(parse_num(&json!(true)), None);
        assert_eq!(parse_num(&json!("nan")), None);
    }

    #[test]
    fn composite_by_overlap() {
        let s = |a: f64, b: f64, v: &[(&str, f64)]| Sample {
            from: json!(a),
            to: json!(b),
            values: v.iter().map(|(n, x)| (n.to_string(), *x)).collect(),
        };
        let samples = [
            s(0.0, 2.0, &[("k", 10.0)]),
            s(2.0, 6.0, &[("k", 20.0), ("n", 1.0)]),
        ];
        let comps = vec!["k".to_string(), "n".to_string()];
        let r = &composite(&samples, &[(1.0, 5.0)], &comps)[0];
        assert_eq!(r.length, 4.0);
        assert_eq!(r.values[0], Some((10.0 + 3.0 * 20.0) / 4.0));
        assert_eq!(r.cover, vec![1.0, 0.75]);
        assert_eq!(r.n_samples, 2);
        assert_eq!(r.overlap, 0.0);
    }
}
