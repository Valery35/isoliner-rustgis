// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 4.11 Образец справочника пластов, порт `grid_isolines/tools/plast_template.py`
//! и разбора `plast_reference.py`.
//!
//! Образец Верхнекамского месторождения (`data/plast_reference_vkmks.csv`,
//! копия из поставки модуля) выдаётся таблицей без геометрии, строки по
//! порядку залегания. Читатель терпимый, как в модуле: столбцы узнаются по
//! синонимам, негодные строки считаются и пропускаются.

use serde_json::{Value as J, json};

use crate::args;
use crate::pyfmt::{self, Arg};
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::tools::density_demo::layer;

/// Образец из поставки модуля.
pub const TEMPLATE: &str = include_str!("../../data/plast_reference_vkmks.csv");

/// Синонимы столбцов (`COLUMN_SYNONYMS`).
const SYNONYMS: &[(&str, &[&str])] = &[
    (
        "code",
        &["code", "код", "кодтела", "индекс", "пласт", "plast"],
    ),
    (
        "order",
        &[
            "order",
            "порядок",
            "n",
            "номер",
            "нn",
            "сверхувниз",
            "номерсверхувниз",
        ],
    ),
    (
        "body",
        &["body", "тело", "телопластмеждупластье", "тип", "kind"],
    ),
    (
        "color",
        &["color", "colour", "цвет", "цветизпалитры", "rgb", "hex"],
    ),
    (
        "strata",
        &[
            "strata",
            "толща",
            "состав",
            "толщасостав",
            "толщасоставзаполнить",
            "зона",
        ],
    ),
    ("hatch", &["hatch", "крап", "штриховка", "pattern"]),
    (
        "note",
        &[
            "note",
            "примечание",
            "примечаниезаполнить",
            "коммент",
            "comment",
        ],
    ),
];

/// Имя столбца к сопоставимому виду: нижний регистр, только буквы и цифры.
fn norm_key(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| {
            c.is_ascii_digit() || c.is_ascii_lowercase() || ('а'..='я').contains(c) || *c == 'ё'
        })
        .collect()
}

/// Вид тела: пласт, междупластье или ничего.
fn norm_body(s: &str) -> Option<bool> {
    let t: String = s
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || ('а'..='я').contains(c) || *c == 'ё')
        .collect();
    if t.is_empty() {
        return None;
    }
    let inter = [
        "междупластье",
        "межпластье",
        "interbed",
        "interlayer",
        "между",
    ];
    if inter.iter().any(|k| t.starts_with(k)) {
        return Some(true);
    }
    let bed = ["пласт", "bed", "seam", "layer"];
    bed.iter().any(|k| t.starts_with(k)).then_some(false)
}

/// Цвет к `#rrggbb` или ничего.
fn norm_color(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches('#').to_lowercase();
    let hex = |t: &str| t.chars().all(|c| c.is_ascii_hexdigit());
    if t.len() == 3 && hex(&t) {
        return Some(format!(
            "#{}",
            t.chars().flat_map(|c| [c, c]).collect::<String>()
        ));
    }
    (t.len() == 6 && hex(&t)).then(|| format!("#{t}"))
}

/// Строки CSV с кавычками, разделитель по первой строке (`;` или `,`).
fn read_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let head = text.lines().next().unwrap_or("");
    let sep = if head.contains(';') && !head.contains(',') {
        ';'
    } else {
        ','
    };
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (c, false) if c == sep => row.push(std::mem::take(&mut cell)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (c, _) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// Запись справочника.
struct Bed {
    code: String,
    order: i64,
    interbed: bool,
    color: Option<String>,
    strata: String,
    note: String,
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let rows = read_csv(TEMPLATE);
    let Some((head, body)) = rows.split_first() else {
        return Err(ToolError::new("в справочнике нет строк", args![]));
    };
    // первое написание столбца на приведённый вид, первый подходящий синоним
    let mut by_norm: Vec<(String, usize)> = Vec::new();
    for (k, name) in head.iter().enumerate() {
        let nk = norm_key(name);
        if !by_norm.iter().any(|(n, _)| *n == nk) {
            by_norm.push((nk, k));
        }
    }
    let col = |ours: &str| -> Option<usize> {
        let syns = SYNONYMS.iter().find(|(o, _)| *o == ours)?.1;
        syns.iter()
            .find_map(|s| by_norm.iter().find(|(n, _)| n == s).map(|(_, k)| *k))
    };
    let (Some(ci), Some(oi), Some(bi)) = (col("code"), col("order"), col("body")) else {
        return Err(ToolError::new("в справочнике не найдены столбцы", args![]));
    };
    let get = |r: &Vec<String>, k: Option<usize>| -> String {
        k.and_then(|k| r.get(k))
            .map_or(String::new(), |s| s.trim().to_string())
    };
    let (mut total, mut kept) = (0usize, 0usize);
    let (mut no_code, mut bad_order, mut bad_body, mut dup) = (0usize, 0usize, 0usize, 0usize);
    let mut beds: Vec<Bed> = Vec::new();
    for r in body {
        total += 1;
        let code = get(r, Some(ci));
        if code.is_empty() {
            no_code += 1;
            continue;
        }
        let Some(order) = get(r, Some(oi))
            .parse::<f64>()
            .ok()
            .filter(|x| x.is_finite())
        else {
            bad_order += 1;
            continue;
        };
        let Some(interbed) = norm_body(&get(r, Some(bi))) else {
            bad_body += 1;
            continue;
        };
        if beds.iter().any(|b| b.code == code) {
            dup += 1;
            continue;
        }
        beds.push(Bed {
            code,
            order: order.trunc() as i64,
            interbed,
            color: norm_color(&get(r, col("color"))),
            strata: get(r, col("strata")),
            note: get(r, col("note")),
        });
        kept += 1;
    }
    beds.sort_by_key(|b| b.order);
    let tr = |s: &str| v.tool.translate(s, v.lang).to_string();
    report.info(
        "%s",
        args![pyfmt::format(
            &tr("Справочник: принято %d из %d строк."),
            &[Arg::from(kept), Arg::from(total)]
        )],
    );
    let parts: Vec<String> = [
        (no_code, "без кода: %d"),
        (bad_order, "без номера порядка: %d"),
        (bad_body, "без вида тела: %d"),
        (dup, "повторов кода (взят первый): %d"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, t)| pyfmt::format(&tr(t), &[Arg::from(*n)]))
    .collect();
    if !parts.is_empty() {
        report.info(
            "%s",
            args![pyfmt::format(
                &tr("Пропущено: %s."),
                &[Arg::from(parts.join(", "))]
            )],
        );
    }
    let feats: Vec<J> = beds
        .iter()
        .map(|b| {
            let vals = json!([
                b.code,
                b.order,
                if b.interbed {
                    tr("междупластье")
                } else {
                    tr("пласт")
                },
                b.color.clone().unwrap_or_default(),
                b.strata,
                b.note,
            ]);
            json!({"geometry": null, "values": vals, "raw": vals})
        })
        .collect();
    report.info(
        "Образец положен в проект. Подайте его в 4.01 и 4.02 как «Справочник пластов (таблица)».",
        args![],
    );
    let fields = [
        ("code", "string"),
        ("order", "int"),
        ("body", "string"),
        ("color", "string"),
        ("strata", "string"),
        ("note", "string"),
    ];
    Ok(json!({"OUTPUT": layer("none", &fields, feats)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_norms() {
        let r = read_csv("a,b\n1,\"x, \"\"y\"\"\"\n");
        assert_eq!(r[1], vec!["1".to_string(), "x, \"y\"".to_string()]);
        assert_eq!(norm_key("№ (сверху вниз)"), "сверхувниз");
        assert_eq!(
            norm_key("Толща / состав (заполнить)"),
            "толщасоставзаполнить"
        );
        assert_eq!(norm_body("Междупластье"), Some(true));
        assert_eq!(norm_body("пласт"), Some(false));
        assert_eq!(norm_color("ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(norm_color("#12345"), None);
    }
}
