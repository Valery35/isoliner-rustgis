// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Подстановка в шаблон по правилам оператора `%` Python. Строки
//! журнала модуля пишутся шаблонами `report.info("...%.4g", x)`, и
//! RustGIS должен показать их слово в слово, со знаком, нулями и
//! числом знаков. Подстановка крейта (`rustgis_kit::i18n::fill`) берёт
//! аргументы готовыми строками, поэтому числа форматируются здесь.

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

#[derive(Default)]
struct Spec {
    plus: bool,
    space: bool,
    left: bool,
    zero: bool,
    alt: bool,
    width: usize,
    prec: Option<usize>,
}

/// `template % tuple(args)`. Без аргументов шаблон идёт как есть: так
/// делает отчёт модуля (`template % args if args else template`).
pub fn format(template: &str, args: &[Arg]) -> String {
    if args.is_empty() {
        return template.to_string();
    }
    let mut out = String::new();
    let mut it = args.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut s = Spec::default();
        while let Some(&f) = chars.peek() {
            match f {
                '+' => s.plus = true,
                ' ' => s.space = true,
                '-' => s.left = true,
                '0' => s.zero = true,
                '#' => s.alt = true,
                _ => break,
            }
            chars.next();
        }
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
            s.width = s.width * 10 + d as usize;
            chars.next();
        }
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut p = 0;
            while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                p = p * 10 + d as usize;
                chars.next();
            }
            s.prec = Some(p);
        }
        let Some(conv) = chars.next() else {
            out.push('%');
            break;
        };
        if conv == '%' {
            out.push('%');
            continue;
        }
        let arg = it.next();
        out.push_str(&convert(conv, &s, arg));
    }
    out
}

fn as_f64(a: Option<&Arg>) -> f64 {
    match a {
        Some(Arg::Int(i)) => *i as f64,
        Some(Arg::Float(f)) => *f,
        Some(Arg::Text(t)) => t.parse().unwrap_or(f64::NAN),
        None => f64::NAN,
    }
}

fn convert(conv: char, s: &Spec, arg: Option<&Arg>) -> String {
    let body = match conv {
        's' | 'r' => {
            let t = match arg {
                Some(Arg::Text(t)) => t.clone(),
                Some(Arg::Int(i)) => i.to_string(),
                Some(Arg::Float(f)) => repr(*f),
                None => String::new(),
            };
            let t = match s.prec {
                Some(p) => t.chars().take(p).collect(),
                None => t,
            };
            return pad(t, s, false);
        }
        'd' | 'i' | 'u' => {
            let v = match arg {
                Some(Arg::Int(i)) => *i,
                other => as_f64(other).trunc() as i64,
            };
            sign(v < 0, s) + &v.unsigned_abs().to_string()
        }
        'f' | 'F' => {
            let v = as_f64(arg);
            let p = s.prec.unwrap_or(6);
            signed(v, s, |a| special(a).unwrap_or_else(|| format!("{a:.p$}")))
        }
        'e' | 'E' => {
            let v = as_f64(arg);
            let p = s.prec.unwrap_or(6);
            let t = signed(v, s, |a| special(a).unwrap_or_else(|| exp(a, p)));
            if conv == 'E' { t.to_uppercase() } else { t }
        }
        'g' | 'G' => {
            let v = as_f64(arg);
            let p = s.prec.unwrap_or(6);
            let t = signed(v, s, |a| special(a).unwrap_or_else(|| general(a, p, s.alt)));
            if conv == 'G' { t.to_uppercase() } else { t }
        }
        _ => return format!("%{conv}"),
    };
    pad(body, s, true)
}

fn special(a: f64) -> Option<String> {
    if a.is_nan() {
        Some("nan".into())
    } else if a.is_infinite() {
        Some("inf".into())
    } else {
        None
    }
}

fn sign(neg: bool, s: &Spec) -> String {
    if neg {
        "-".into()
    } else if s.plus {
        "+".into()
    } else if s.space {
        " ".into()
    } else {
        String::new()
    }
}

fn signed(v: f64, s: &Spec, body: impl Fn(f64) -> String) -> String {
    let neg = v.is_sign_negative() && !v.is_nan();
    sign(neg, s) + &body(v.abs())
}

/// Ширина поля: пробелы слева, справа при `-` или нули после знака.
fn pad(t: String, s: &Spec, numeric: bool) -> String {
    let len = t.chars().count();
    if len >= s.width {
        return t;
    }
    let fill = s.width - len;
    if s.left {
        t + &" ".repeat(fill)
    } else if s.zero && numeric && !t.ends_with("nan") && !t.ends_with("inf") {
        let (head, tail) = match t.chars().next() {
            Some(c @ ('+' | '-' | ' ')) => (c.to_string(), t[1..].to_string()),
            _ => (String::new(), t),
        };
        head + &"0".repeat(fill) + &tail
    } else {
        " ".repeat(fill) + &t
    }
}

/// Мантисса и порядок как у `%e` Python: `1.234500e+05`.
fn exp(a: f64, p: usize) -> String {
    let r = format!("{a:.p$e}");
    let (m, e) = r.split_once('e').unwrap_or((&r, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    format!(
        "{m}e{}{:02}",
        if e < 0 { '-' } else { '+' },
        e.unsigned_abs()
    )
}

/// `%g`: точность - значащие цифры, хвостовые нули снимаются.
fn general(a: f64, p: usize, alt: bool) -> String {
    let p = p.max(1);
    if a == 0.0 {
        return if alt {
            format!("{:.*}", p - 1, 0.0)
        } else {
            "0".into()
        };
    }
    // порядок после округления до p значащих цифр
    let r = format!("{a:.*e}", p - 1);
    let x: i32 = r
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    let t = if x >= -4 && x < p as i32 {
        let decimals = (p as i32 - 1 - x).max(0) as usize;
        format!("{a:.decimals$}")
    } else {
        exp(a, p - 1)
    };
    if alt { t } else { strip_zeros(&t) }
}

fn strip_zeros(t: &str) -> String {
    let (m, e) = match t.find('e') {
        Some(i) => (&t[..i], &t[i..]),
        None => (t, ""),
    };
    let m = if m.contains('.') {
        m.trim_end_matches('0').trim_end_matches('.')
    } else {
        m
    };
    format!("{m}{e}")
}

/// `repr` числа Python: кратчайшая запись, целое - с `.0`.
fn repr(f: f64) -> String {
    if let Some(s) = special(f.abs()) {
        return if f.is_sign_negative() && f.is_infinite() {
            format!("-{s}")
        } else {
            s
        };
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

    /// Ожидания взяты из Python 3: `"шаблон" % (аргументы)`.
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
        // без аргументов шаблон не трогается, как в отчёте модуля
        assert_eq!(f("100%% и %d", &[]), "100%% и %d");
    }
}
