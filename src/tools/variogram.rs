// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.06 Вариограмма (экспериментальная), порт `grid_isolines/tools/variogram.py`
//! и `tools/vario.py`.
//!
//! Выходы - две таблицы без геометрии: OUTPUT (серия, расстояние, γ,
//! количество пар) и OUT_MODEL (модель вариограммы, строка на
//! структуру). Облака пар, наложения заданной модели и записи в чужую
//! таблицу моделей здесь нет, это работа окна QGIS.

use serde_json::{Value as J, json};

use crate::args;
use crate::field_format::{plan_for, row};
use crate::kb2d::{self, DataWarning, Experimental, Fit};
use crate::numpy;
use crate::points::{Outliers, Points, read_points};
use crate::pyfmt::Arg;
use crate::report::{Report, ToolError};
use crate::tools::Values;

/// Подписи полей выходов (`FIELD_LABELS` класса).
pub const FIELD_LABELS: &[(&str, &str)] = &[
    ("series", "Серия"),
    ("lag", "Лаг (расстояние)"),
    ("gamma", "Полувариограмма γ"),
    ("npairs", "Пар точек"),
    ("profile", "Профиль обработки"),
    ("field", "Поле значения"),
    ("struct", "Номер структуры"),
    ("model", "Модель (код)"),
    ("sill", "Силл"),
    ("range", "Радиус влияния"),
    ("azimuth", "Азимут, °"),
    ("anis", "Анизотропия"),
    ("nugget", "Наггет C0"),
    ("val_pct", "Перцентиль обрезки проб, %"),
    ("val_cap", "Срезать к границе"),
    ("fitted", "Дата подбора"),
    ("author", "Автор"),
    ("note", "Примечание"),
];

/// Названия моделей подбора по коду с нуля.
pub const MODEL_NAMES: [&str; 4] = ["Сферическая", "Экспоненциальная", "Гауссова", "Степенная"];

pub const MODEL_NOTE: &str =
    "автоподбор 1.06, изотропная. Анизотропию проставляет 1.07 по вариограммной карте";

const MODEL_FIELDS: [(&str, &str); 14] = [
    ("profile", "string"),
    ("field", "string"),
    ("struct", "int"),
    ("model", "int"),
    ("sill", "double"),
    ("range", "double"),
    ("azimuth", "double"),
    ("anis", "double"),
    ("nugget", "double"),
    ("val_pct", "double"),
    ("val_cap", "int"),
    ("fitted", "string"),
    ("author", "string"),
    ("note", "string"),
];

/// Границы отсева ураганных проб из полей окна.
pub fn outliers(v: &Values<'_>) -> Outliers {
    let opt = |k: &str| match v.get(k) {
        J::Null => None,
        J::String(s) if s.trim().is_empty() => None,
        x => crate::points::num(&x),
    };
    Outliers {
        vmin: opt("VAL_MIN"),
        vmax: opt("VAL_MAX"),
        pct: {
            let p = v.number("VAL_PCT");
            if p.is_nan() { 0.0 } else { p }
        },
        cap: v.flag("VAL_CAP"),
    }
}

/// Сообщения о данных (`vario.warn_data`).
pub fn warn_data(report: &mut Report, p: &Points) {
    for w in kb2d::data_warnings(&p.xs, &p.ys, &p.vs) {
        match w {
            DataWarning::FewPoints(n) => report.warn(
                "Мало точек (%d): оценка кригинга и вариограммы неустойчива.",
                args![n],
            ),
            DataWarning::Duplicates(n) => report.warn(
                "Точек с совпадающими координатами: %d. Частая причина вырожденной \
                 матрицы и артефактов. Уберите дубли или усредните пробы в одной точке.",
                args![n],
            ),
            DataWarning::Constant => report.warn(
                "Все значения одинаковы. Кригинг вырождается, вариограмма нулевая. \
                 Проверьте выбранное поле.",
                args![],
            ),
        }
    }
}

/// Сообщения о качестве подбора (`vario.warn_fit_quality`).
pub fn warn_fit_quality(report: &mut Report, fit: &Fit) {
    let (r2, c0) = (fit.r2, fit.nugget);
    let total = c0 + fit.sill;
    if r2 < 0.1 {
        report.warn(
            "Качество подгонки R²=%.3f ниже %.2g: модель почти ничего не объясняет. \
             Пользоваться её параметрами как есть нельзя - проверьте выбросы и \
             парные близкие точки.",
            args![r2, 0.1],
        );
    }
    if total > 0.0 && c0 / total > 0.5 {
        report.warn(
            "Доля наггета %.0f%% от суммарного порога. Связь на коротких расстояниях \
             не разрешена. Кригинг по такой модели сглаживает оценку до среднего, а \
             на карте дают «бычьи глаза».",
            args![100.0 * c0 / total],
        );
    }
}

/// Первый шаг и пары, формирующие наггет (`vario.report_nugget_pairs`).
pub fn report_nugget_pairs(report: &mut Report, p: &Points, ev: &Experimental, data_var: f64) {
    if ev.lag.is_empty() || ev.gamma.is_empty() {
        return;
    }
    let (h1, g1) = (ev.lag[0], ev.gamma[0]);
    let n1 = ev.npairs.first().copied().unwrap_or(0);
    report.info(
        "Первый шаг: h=%.4g, пар %d, γ=%.4g при дисперсии данных %.4g.",
        args![h1, n1, g1, data_var],
    );
    if data_var > 0.0 && g1 > data_var {
        report.warn(
            "На первом шаге разброс уже выше общей дисперсии. Описать это можно только \
             наггетом, и кригинг после такого подбора будет возвращать среднее вместо карты.",
            args![],
        );
    }
    let pairs = kb2d::nugget_pairs(&p.xs, &p.ys, &p.vs, h1, 5);
    if pairs.is_empty() {
        return;
    }
    report.info("Пары, формирующие наггет (внутри первого шага):", args![]);
    for (i, j, dist, vi, vj, g) in pairs {
        report.info(
            "  расстояние %.4g, значения %.4g и %.4g, вклад γ=%.4g (x %.6g y %.6g / x %.6g y %.6g)",
            args![dist, vi, vj, g, p.xs[i], p.ys[i], p.xs[j], p.ys[j]],
        );
    }
}

/// Сегодняшняя дата ISO (UTC).
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // дни от 1970-01-01 в дату (алгоритм Хиннанта)
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Веса декластеризации по точкам: все конечные и положительные, иначе
/// `Err(true)` (есть пустые или неположительные) или `Err(false)` (не
/// читаются числом).
fn weights(ids: &[J]) -> Result<Vec<f64>, bool> {
    let mut out = Vec::with_capacity(ids.len());
    for w in ids {
        let x = match w {
            J::Null => f64::NAN,
            J::Bool(b) => f64::from(u8::from(*b)),
            J::Number(n) => n.as_f64().unwrap_or(f64::NAN),
            J::String(s) => s.trim().parse().map_err(|_| false)?,
            _ => return Err(false),
        };
        out.push(x);
    }
    if out.iter().all(|x| x.is_finite() && *x > 0.0) {
        Ok(out)
    } else {
        Err(true)
    }
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let items = v.items("INPUT");
    let layer = v.layer("INPUT");
    let zfield = v.text("ZFIELD");
    let gfield = v.text("GROUP_FIELD");
    let min_group_pct = {
        let x = v.number("MIN_GROUP_PCT");
        if x.is_nan() { 0.0 } else { x }
    };
    let n_lags = v.integer("N_LAGS");
    let maxlag = {
        let x = v.number("MAXLAG");
        if x.is_nan() { 0.0 } else { x }
    };
    let robust = v.flag("ROBUST");
    let do_fit = v.flag("FIT");
    let fit_choice = v.integer("FIT_MODEL");
    let clip = outliers(v);
    let wfield = v.text("WEIGHT_FIELD");

    let mut wts: Option<Vec<f64>> = None;
    let p = if wfield.is_empty() {
        read_points(items, &zfield, layer, report, clip, None)?
    } else {
        let p = read_points(items, &zfield, layer, report, clip, Some(&wfield))?;
        match weights(&p.ids) {
            Ok(w) => {
                wts = Some(w);
                report.info(
                    "Пары взвешены декластеризацией (поле «%s»).",
                    args![wfield.as_str()],
                );
            }
            Err(true) => report.warn(
                "Поле весов содержит пустые или неположительные значения - веса игнорируются.",
                args![],
            ),
            Err(false) => report.warn(
                "Не удалось прочитать поле весов - веса игнорируются.",
                args![],
            ),
        }
        p
    };
    warn_data(report, &p);
    let data_var = numpy::var(&p.vs);
    report.info(
        "Точек: %d. Дисперсия данных: %.4g (ориентир для суммарного порога).",
        args![p.xs.len(), data_var],
    );
    let group_min = ((min_group_pct / 100.0 * p.xs.len() as f64).round_ties_even() as i64).max(30);
    let ev =
        kb2d::experimental_variogram(&p.xs, &p.ys, &p.vs, n_lags, maxlag, robust, wts.as_deref());
    if ev.subsampled {
        report.info(
            "Точек много - для расчёта пар использована случайная подвыборка %d точек.",
            args![ev.n_used],
        );
    }
    report_nugget_pairs(report, &p, &ev, data_var);
    if maxlag > 0.0 {
        let span = |a: &[f64]| {
            a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - a.iter().copied().fold(f64::INFINITY, f64::min)
        };
        let (w, h) = (span(&p.xs), span(&p.ys));
        let spacing = if w > 0.0 && h > 0.0 {
            (w * h / p.xs.len().max(1) as f64).powf(0.5)
        } else {
            0.0
        };
        if spacing > 0.0 && maxlag < spacing {
            report.warn(
                "Максимальное расстояние (%.4g) меньше типичного шага между точками \
                 (~%.4g) - пар почти нет. Значение задаётся в единицах слоя (обычно метры).",
                args![maxlag, spacing],
            );
        }
    }

    // серии: общая кривая и по группам
    let all = v.tool.translate("все точки", v.lang).to_string();
    let empty_word = v.tool.translate("(пусто)", v.lang).to_string();
    let mut series: Vec<(String, Experimental)> = vec![(all, ev.clone())];
    if !gfield.is_empty() {
        let key = |g: &J| {
            if g.is_null() {
                String::new()
            } else {
                crate::drillhole::py_str(g)
            }
        };
        let mut vals: Vec<J> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for it in items {
            let g = &it["attrs"][gfield.as_str()];
            let k = key(g);
            if !seen.contains(&k) {
                seen.push(k);
                vals.push(g.clone());
            }
            if seen.len() > 12 {
                break;
            }
        }
        if vals.len() > 12 {
            report.warn("Групп больше 12 - группировка пропущена.", args![]);
        } else if vals.len() >= 2 {
            let mut skipped: Vec<String> = Vec::new();
            for g in &vals {
                let sub: Vec<J> = items
                    .iter()
                    .filter(|it| {
                        let a = &it["attrs"][gfield.as_str()];
                        if g.is_null() {
                            a.is_null()
                        } else {
                            !a.is_null()
                                && crate::drillhole::py_str(a) == crate::drillhole::py_str(g)
                        }
                    })
                    .cloned()
                    .collect();
                let mut quiet = Report::default();
                let Ok(gp) = read_points(&sub, &zfield, layer, &mut quiet, clip, None) else {
                    continue;
                };
                let label = if g.is_null() {
                    empty_word.clone()
                } else {
                    crate::drillhole::py_str(g)
                };
                if (gp.xs.len() as i64) < group_min {
                    skipped.push(format!("{label} ({})", gp.xs.len()));
                    continue;
                }
                let gev = kb2d::experimental_variogram(
                    &gp.xs, &gp.ys, &gp.vs, n_lags, ev.maxlag, robust, None,
                );
                series.push((format!("{gfield} = {label}"), gev));
            }
            if !skipped.is_empty() {
                report.info(
                    "Группы меньше %d точек пропущены: %s.",
                    args![group_min, skipped.join(", ")],
                );
            }
        } else {
            report.info(
                "В поле группировки меньше 2 значений - строю только общую кривую.",
                args![],
            );
        }
    }

    // подбор модели по общей кривой
    let mut fit = None;
    if do_fit {
        let model = if fit_choice == 0 {
            None
        } else {
            Some((fit_choice - 1) as usize)
        };
        fit = kb2d::fit_variogram(&ev.lag, &ev.gamma, &ev.npairs, model);
        if let Some(f) = &fit {
            report.info(
                "Подбор: модель %s, C0=%.4g, C=%.4g, a=%.4g, R²=%.3f",
                vec![
                    Arg::from(MODEL_NAMES[f.model.min(3)]),
                    f.nugget.into(),
                    f.sill.into(),
                    f.range.into(),
                    f.r2.into(),
                ],
            );
            warn_fit_quality(report, f);
        }
    }

    let skip = v.input_fields();
    let names = ["series", "lag", "gamma", "npairs"];
    let plan = plan_for(&names, FIELD_LABELS, &[], &skip);
    let mut feats = Vec::new();
    for (label, s) in &series {
        for i in 0..s.lag.len() {
            let (values, raw) = row(
                &names,
                vec![
                    json!(label),
                    json!(s.lag[i]),
                    json!(s.gamma[i]),
                    json!(s.npairs[i]),
                ],
                &plan,
            );
            feats.push(json!({"geometry": null, "values": values, "raw": raw}));
        }
    }
    let fields: Vec<J> = [
        ("series", "string"),
        ("lag", "double"),
        ("gamma", "double"),
        ("npairs", "int"),
    ]
    .iter()
    .map(|(n, t)| json!({"name": n, "type": t}))
    .collect();
    let mut out = json!({"OUTPUT": {"geometry": "none", "fields": fields, "features": feats}});

    if let Some(f) = fit
        && v.values["OUT_MODEL"] != J::Bool(false)
    {
        let name = v.text("SAVE_PROFILE").trim().to_string();
        let profile = if name.is_empty() {
            v.tool.translate("без имени", v.lang).to_string()
        } else {
            name
        };
        let mnames: Vec<&str> = MODEL_FIELDS.iter().map(|(n, _)| *n).collect();
        let mplan = plan_for(&mnames, FIELD_LABELS, &[], &skip);
        let (values, raw) = row(
            &mnames,
            vec![
                json!(profile),
                json!(zfield),
                json!(1),
                json!(f.model + 1),
                json!(f.sill),
                json!(f.range),
                json!(0.0),
                json!(1.0),
                json!(f.nugget),
                json!(clip.pct),
                json!(i64::from(clip.cap)),
                json!(today()),
                json!(""),
                json!(v.tool.translate(MODEL_NOTE, v.lang)),
            ],
            &mplan,
        );
        let mfields: Vec<J> = MODEL_FIELDS
            .iter()
            .map(|(n, t)| json!({"name": n, "type": t}))
            .collect();
        out["OUT_MODEL"] = json!({
            "geometry": "none",
            "fields": mfields,
            "features": [{"geometry": null, "values": values, "raw": raw}],
        });
    }
    Ok(out)
}
