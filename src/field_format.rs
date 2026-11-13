// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Точность чисел в выходных полях, как `grid_isolines/field_format.py`
//! модуля: `decimals_for` по подписи поля, `plan_for` по полям выхода,
//! `round_value`. Слой в RustGIS несёт те же числа, что слой модуля в
//! QGIS.

use std::collections::HashMap;

use serde_json::{Value as J, json};

use crate::pyfmt::{self, Arg};

/// Точность поля: знаков после запятой или значащих цифр.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec {
    Decimals(i32),
    Significant(usize),
}

/// Слово кончается: после совпадения не буква, не цифра и не `_`
/// (`\b` после буквы в `re` Python).
fn word_end(label: &str, pat: &str) -> bool {
    label.match_indices(pat).any(|(i, m)| {
        label[i + m.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
}

/// Правило: узнаёт подпись и даёт точность.
type Rule = (fn(&str) -> bool, Spec);

/// Правила `_RULES` модуля в том же порядке: первое совпадение решает.
fn rules() -> Vec<Rule> {
    use Spec::{Decimals as D, Significant as G};
    vec![
        (|l| l.contains("10⁻⁶ 1/м"), D(2)),
        (|l| word_end(l, "мм/м"), D(3)),
        (|l| l.contains("мм/год"), D(1)),
        (|l| word_end(l, ", мм"), D(1)),
        (|l| l.contains("м/сут") || l.contains("м²/сут"), G(4)),
        (|l| l.contains("м³/с"), D(3)),
        (|l| word_end(l, "м/с"), D(3)),
        (|l| l.contains("км²"), D(3)),
        (|l| word_end(l, ", км"), D(3)),
        (|l| l.contains("м³"), D(1)),
        (|l| l.contains("м²"), D(1)),
        (|l| l.contains("Ом·м"), D(2)),
        (|l| l.contains("мВ/В"), D(2)),
        (|l| word_end(l, "мВ"), D(1)),
        (
            |l| {
                ["доля", "Вероятност", "R²", "Плоскостность", "Шероховатость"]
                    .iter()
                    .any(|p| l.contains(p))
            },
            D(4),
        ),
        (|l| l.contains("градус") || l.contains('°'), D(1)),
        (|l| l.contains('%'), D(2)),
        (
            |l| {
                l.contains("м/м")
                    || l.ends_with("Уклон")
                    || l.ends_with("уклон")
                    || l.contains("Градиент")
                    || l.contains("Удельный расход")
                    || l.contains("перепад на ячейку")
            },
            G(4),
        ),
        (|l| word_end(l, ", м"), D(2)),
        (|l| l.contains("Размерность"), D(3)),
        (|l| l.starts_with("Вес"), G(4)),
        (|l| l.contains("Станд. остаток"), D(3)),
        (
            |l| {
                ["Полувариограмма", "Силл", "Наггет", "Радиус влияния", "Лаг"]
                    .iter()
                    .any(|p| l.contains(p))
            },
            G(5),
        ),
        (
            |l| l.contains("Анизотропия") || l.contains("Вертикальное преувеличение"),
            D(3),
        ),
        (|l| l.contains("Масса"), G(6)),
        (|l| l.contains("координата z"), D(4)),
        (|l| l.contains("Амплитуда"), D(2)),
        (|l| l.contains("Значение для дрейфа"), D(3)),
        (
            |l| l == "X" || l == "Y" || l.contains("Сдвиг чертежа"),
            D(2),
        ),
        (|l| l.contains("Зазор"), D(3)),
        (|l| l == "Площадь сечения", D(2)),
        (|l| l.starts_with("Пикет"), D(3)),
        (|l| l.contains("Среднее значение"), G(5)),
    ]
}

/// Точность по подписи поля или `None`, если единиц в подписи нет.
pub fn decimals_for(label: &str) -> Option<Spec> {
    if label.is_empty() {
        return None;
    }
    rules().into_iter().find(|(m, _)| m(label)).map(|(_, s)| s)
}

/// Точность по именам полей (`plan_for`). `labels` - FIELD_LABELS
/// инструмента, ключ с `*` на конце - поля с переменным хвостом.
/// `overrides` - FIELD_DECIMALS. Поля из `skip` (поля входов) не
/// округляются: это данные пользователя.
pub fn plan_for(
    names: &[&str],
    labels: &[(&str, &str)],
    overrides: &[(&str, Spec)],
    skip: &[String],
) -> HashMap<String, Spec> {
    let mut out = HashMap::new();
    for &nm in names {
        if skip.iter().any(|s| s == nm) {
            continue;
        }
        let spec = match overrides.iter().find(|(k, _)| *k == nm) {
            Some((_, s)) => Some(*s),
            None => labels
                .iter()
                .find(|(k, _)| *k == nm)
                .or_else(|| {
                    labels
                        .iter()
                        .find(|(k, _)| k.strip_suffix('*').is_some_and(|p| nm.starts_with(p)))
                })
                .and_then(|(_, l)| decimals_for(l)),
        };
        if let Some(s) = spec {
            out.insert(nm.to_string(), s);
        }
    }
    out
}

/// Округлить число. Не конечное возвращается как есть.
pub fn round_value(v: f64, spec: Spec) -> f64 {
    if !v.is_finite() {
        return v;
    }
    let r = match spec {
        // round(value, n) Python: точное десятичное округление половины к чётному
        Spec::Decimals(n) => {
            let n = n.max(0) as usize;
            format!("{v:.n$}").parse().unwrap_or(v)
        }
        Spec::Significant(n) => {
            if v == 0.0 {
                return 0.0;
            }
            pyfmt::format(&format!("%.{n}g"), &[Arg::Float(v)])
                .parse()
                .unwrap_or(v)
        }
    };
    // + 0.0 модуля: минус ноль - в ноль
    r + 0.0
}

/// Строка выхода: `values` после округления по плану и `raw` как есть.
/// Округляются только дробные числа, целые и строки идут как есть.
pub fn row(names: &[&str], raw: Vec<J>, plan: &HashMap<String, Spec>) -> (J, J) {
    let values: Vec<J> = names
        .iter()
        .zip(&raw)
        .map(|(n, v)| match (plan.get(*n), v.as_f64()) {
            (Some(s), Some(x)) if v.is_f64() => json!(round_value(x, *s)),
            _ => v.clone(),
        })
        .collect();
    (J::Array(values), J::Array(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_module() {
        assert_eq!(
            round_value(0.544_952_876_984_127_1, Spec::Significant(4)),
            0.545
        );
        assert_eq!(
            round_value(1.416_666_666_666_666_7, Spec::Significant(4)),
            1.417
        );
        assert_eq!(round_value(123_456.7, Spec::Significant(4)), 123_500.0);
        assert_eq!(round_value(2.675, Spec::Decimals(2)), 2.67);
        assert_eq!(
            round_value(-0.0001, Spec::Decimals(2)).to_bits(),
            0.0f64.to_bits()
        );
        assert!(round_value(f64::NAN, Spec::Decimals(2)).is_nan());
    }

    /// Ожидания - `decimals_for` модуля на тех же подписях.
    #[test]
    fn rules_like_module() {
        use Spec::{Decimals as D, Significant as G};
        let cases: [(&str, Option<Spec>); 16] = [
            ("Вес декластеризации", Some(G(4))),
            ("Пикет, м", Some(D(2))),
            ("Пикет (ПК)", Some(D(3))),
            ("Отметка z, м", Some(D(2))),
            ("ρк, Ом·м", Some(D(2))),
            ("ЕП, мВ", Some(D(1))),
            ("ВП, мВ/В", Some(D(2))),
            ("Оседание, мм", Some(D(1))),
            ("Наклон, мм/м", Some(D(3))),
            ("Кривизна, 10⁻⁶ 1/м", Some(D(2))),
            ("Длина, км", Some(D(3))),
            ("Профиль", None),
            ("Тур", None),
            ("Расход, м³/с", Some(D(3))),
            ("Объём, м³", Some(D(1))),
            ("Мощность, мера", None),
        ];
        for (l, want) in cases {
            assert_eq!(decimals_for(l), want, "{l}");
        }
        let plan = plan_for(
            &["a", "cov_x", "wt", "z"],
            &[
                ("wt", "Вес"),
                ("cov_*", "Охват по %s, доля"),
                ("z", "Отметка, м"),
            ],
            &[("a", D(1))],
            &["z".to_string()],
        );
        assert_eq!(plan.get("a"), Some(&D(1)));
        assert_eq!(plan.get("cov_x"), Some(&D(4)));
        assert_eq!(plan.get("wt"), Some(&G(4)));
        assert_eq!(plan.get("z"), None);
    }
}
