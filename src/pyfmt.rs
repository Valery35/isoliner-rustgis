// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Подстановка в шаблон по правилам оператора `%` Python. Строки
//! журнала модуля пишутся шаблонами `report.info("...%.4g", x)`, и
//! RustGIS должен показать их слово в слово. Форматы исполняет крейт
//! (`rustgis_kit::i18n::fill`, с 0.1.2). Здесь - типы аргументов, запись
//! числа строкой как у Python и правило шаблона без аргументов.

/// Аргумент шаблона.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i64),
    Float(f64),
    Text(String),
}

impl From<i64> for Arg {
    fn from(v: i64) -> Self {
        Arg::Int(v)
    }
}

impl From<usize> for Arg {
    fn from(v: usize) -> Self {
        Arg::Int(i64::try_from(v).unwrap_or(i64::MAX))
    }
}

impl From<f64> for Arg {
    fn from(v: f64) -> Self {
        Arg::Float(v)
    }
}

impl From<&str> for Arg {
    fn from(v: &str) -> Self {
        Arg::Text(v.to_string())
    }
}

impl From<String> for Arg {
    fn from(v: String) -> Self {
        Arg::Text(v)
    }
}

impl Arg {
    /// Аргумент строкой для крейта: число - как `str()` Python.
    pub fn text(&self) -> String {
        match self {
            Arg::Int(i) => i.to_string(),
            Arg::Float(f) => repr(*f),
            Arg::Text(t) => t.clone(),
        }
    }
}

/// `template % tuple(args)`. Без аргументов шаблон идёт как есть: так
/// делает отчёт модуля (`template % args if args else template`).
pub fn format(template: &str, args: &[Arg]) -> String {
    if args.is_empty() {
        return template.to_string();
    }
    let args: Vec<String> = args.iter().map(Arg::text).collect();
    rustgis_kit::i18n::fill(template, &args)
}

/// `repr` числа Python: кратчайшая запись, целое - с `.0`.
fn repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-inf" } else { "inf" }.into();
    }
    let a = f.abs();
    if a != 0.0 && !(1e-4..1e16).contains(&a) {
        // Python: 1e-05, 1.5e+16
        let r = format!("{f:e}");
        let (m, e) = r.split_once('e').unwrap_or((&r, "0"));
        let e: i32 = e.parse().unwrap_or(0);
        return format!(
            "{m}e{}{:02}",
            if e < 0 { '-' } else { '+' },
            e.unsigned_abs()
        );
    }
    let s = format!("{f}");
    if s.contains('.') { s } else { s + ".0" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(t: &str, a: &[Arg]) -> String {
        format(t, a)
    }

    /// Ожидания взяты из Python 3: `"шаблон" % (аргументы)`. Сторож над
    /// форматами крейта: строки журнала обязаны совпадать с модулем.
    #[test]
    fn like_python() {
        assert_eq!(f("%.4g", &[123.456.into()]), "123.5");
        assert_eq!(f("%.4g", &[0.000_123_456.into()]), "0.0001235");
        assert_eq!(f("%.4g", &[0.000_012_345_6.into()]), "1.235e-05");
        assert_eq!(f("%.4g", &[123_456.0.into()]), "1.235e+05");
        assert_eq!(f("%.4g", &[1234.0.into()]), "1234");
        assert_eq!(f("%.4g", &[9999.5.into()]), "1e+04");
        assert_eq!(f("%.4g", &[2.5.into()]), "2.5");
        assert_eq!(f("%.4g", &[0.0.into()]), "0");
        assert_eq!(f("%.4g", &[(-3.0).into()]), "-3");
        assert_eq!(f("%.4g", &[1e-300.into()]), "1e-300");
        assert_eq!(
            f("[%.4g; %.4g]", &[f64::NEG_INFINITY.into(), 10.0.into()]),
            "[-inf; 10]"
        );
        assert_eq!(f("%g", &[100_000.0.into()]), "100000");
        assert_eq!(f("%g", &[1_000_000.0.into()]), "1e+06");
        assert_eq!(f("(%+.2f%%)", &[(-3.25159).into()]), "(-3.25%)");
        assert_eq!(f("(%+.2f%%)", &[0.0.into()]), "(+0.00%)");
        assert_eq!(f("%d из %d", &[3usize.into(), 10usize.into()]), "3 из 10");
        assert_eq!(f("«%s»", &["z".into()]), "«z»");
        assert_eq!(
            f("%5.1f|%-4d|%03d", &[2.25.into(), 7i64.into(), 5i64.into()]),
            "  2.2|7   |005"
        );
        assert_eq!(f("%.3e", &[12345.678.into()]), "1.235e+04");
        assert_eq!(f("%s", &[0.1.into()]), "0.1");
        assert_eq!(f("%s", &[3.0.into()]), "3.0");
        assert_eq!(f("%s", &[1e-5.into()]), "1e-05");
        // без аргументов шаблон не трогается, как в отчёте модуля
        assert_eq!(f("100%% и %d", &[]), "100%% и %d");
    }
}
