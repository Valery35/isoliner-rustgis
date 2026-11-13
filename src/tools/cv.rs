// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Общее для кросс-валидаций 1.08 и 1.09, порт `grid_isolines/tools/cv.py`:
//! вариограмма из таблицы моделей или из полей окна, метрики с весами
//! декластеризации и советы по ним.

use serde_json::Value as J;

use crate::args;
use crate::krige::{GAUSS_MIN_NUGGET_FRAC, Structure, Variogram};
use crate::numpy;
use crate::pyfmt::Arg;
use crate::report::{Report, ToolError, render};
use crate::tools::Values;
use crate::vgtable;

/// Вариограмма из таблицы моделей VG_TABLE, если она подана.
fn from_table(v: &Values<'_>, report: &mut Report) -> Result<Option<Variogram>, ToolError> {
    let J::Array(table) = &v.values["VG_TABLE"] else {
        return Ok(None);
    };
    let name = v.text("VG_NAME").trim().to_string();
    let rows: Vec<J> = table.iter().map(|it| it["attrs"].clone()).collect();
    let (models, notes) = vgtable::parse_rows(&rows);
    for note in notes {
        report.warn("Таблица вариограмм: %s", args![note]);
    }
    if models.is_empty() {
        return Err(ToolError::new(
            "В таблице вариограмм нет ни одной годной модели.",
            args![],
        ));
    }
    let mut names: Vec<&str> = models.iter().map(|m| m.profile.as_str()).collect();
    names.sort_unstable();
    let name = if name.is_empty() {
        if models.len() > 1 {
            return Err(ToolError::new(
                "В таблице несколько профилей (%s). Укажите, какой брать.",
                args![names.join(", ")],
            ));
        }
        models[0].profile.clone()
    } else {
        name
    };
    let Some(m) = models.iter().find(|m| m.profile == name) else {
        return Err(ToolError::new(
            "Профиль «%s» в таблице не найден. Есть: %s.",
            args![name, names.join(", ")],
        ));
    };
    report.info(
        "Вариограмма из таблицы, профиль «%s»: наггет %.4g, структур %d. Поля \
         вариограммы в окне при этом не читаются.",
        args![m.profile.as_str(), m.nugget, m.structs.len()],
    );
    Ok(Some(m.variogram()))
}

/// Вариограмма из таблицы или из полей S1_* окна (`build_variogram`).
pub fn build_variogram(
    v: &Values<'_>,
    report: &mut Report,
    nugget: f64,
    auto_range: f64,
) -> Result<Variogram, ToolError> {
    if let Some(vg) = from_table(v, report)? {
        return Ok(vg);
    }
    let or0 = |x: f64| if x.is_nan() { 0.0 } else { x };
    let sill = or0(v.number("S1_SILL"));
    let model = v.integer("S1_MODEL") + 1;
    let mut rng = or0(v.number("S1_RANGE"));
    if model == 4 {
        if rng <= 0.0 {
            rng = 1.0;
            report.warn(
                "Структура %d: степенная модель - поле «радиус a» это показатель ω \
                 (0<ω<2), а не радиус. Задан 0, взят ω=1.",
                args![1i64],
            );
        } else if !(0.0 < rng && rng < 2.0) {
            report.warn(
                "Структура %d: показатель степенной модели ω=%.3g вне (0; 2) - \
                 приведён к диапазону.",
                args![1i64, rng],
            );
            rng = rng.clamp(0.05, 1.999);
        }
    } else if rng <= 0.0 {
        rng = auto_range;
    }
    let az = or0(v.number("S1_AZIMUTH"));
    let anis = match v.get("S1_ANIS") {
        J::Null => 1.0,
        x => crate::points::num(&x).unwrap_or(1.0),
    };
    let sill_total = sill.max(0.0);
    let s = [Structure {
        it: model,
        cc: sill.max(0.0),
        aa: rng,
        ang: az,
        anis,
    }];
    if nugget > 0.0 && sill_total <= 0.0 {
        report.warn(
            "Задан только наггет (структурный вклад C = 0): кригинг выродится в \
             локальное среднее, поверхность будет почти плоской.",
            args![],
        );
    }
    let vg = Variogram::new(nugget, &s);
    for (num, was) in &vg.power_clamped {
        report.warn(
            "Структура %d: у степенной модели поле «радиус a» это показатель ω \
             (0 < ω < 2), а не расстояние. Задано %.4g, взято %.3f. При показателе \
             в метрах ковариация переполняется, и оценка выходит мусором.",
            args![*num, *was, vg.aa[num - 1]],
        );
    }
    if let Some(was) = vg.nugget_raised_from {
        report.info(
            "Гауссова модель: наггет повышен с %.4g до %.4g для устойчивости \
             (минимум %g%% структурного силла).",
            args![was, vg.c0, GAUSS_MIN_NUGGET_FRAC * 100.0],
        );
    }
    Ok(vg)
}

/// Метрики кросс-валидации.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub me: f64,
    pub mae: f64,
    pub rmse: f64,
    pub msdr: f64,
    pub r: f64,
}

/// Метрики, взвешенные декластеризацией (`weighted_cv_metrics`).
pub fn weighted_cv_metrics(
    fact: &[f64],
    est: &[f64],
    err: &[f64],
    var: &[f64],
    w: Option<&[f64]>,
) -> Metrics {
    let n = err.len();
    let wv: Vec<f64> = w.map_or_else(|| vec![1.0; n], <[f64]>::to_vec);
    let s = |f: &dyn Fn(usize) -> f64| numpy::sum(&(0..n).map(f).collect::<Vec<_>>());
    let ww = {
        let x = numpy::sum(&wv);
        if x == 0.0 { 1.0 } else { x }
    };
    let me = s(&|i| wv[i] * err[i]) / ww;
    let mae = s(&|i| wv[i] * err[i].abs()) / ww;
    let rmse = (s(&|i| wv[i] * err[i] * err[i]) / ww).sqrt();
    let mut msdr = f64::NAN;
    // без дисперсий (минимальная кривизна) MSDR не считается
    let sd: Vec<f64> = var.iter().map(|v| v.max(0.0).sqrt()).collect();
    let good: Vec<usize> = if sd.len() == n {
        (0..n).filter(|&i| sd[i] > 0.0).collect()
    } else {
        Vec::new()
    };
    if !good.is_empty() {
        let num: Vec<f64> = good
            .iter()
            .map(|&i| {
                let q = err[i] / sd[i];
                wv[i] * (q * q)
            })
            .collect();
        let gw: Vec<f64> = good.iter().map(|&i| wv[i]).collect();
        let den = {
            let x = numpy::sum(&gw);
            if x == 0.0 { 1.0 } else { x }
        };
        msdr = numpy::sum(&num) / den;
    }
    let mf = s(&|i| wv[i] * fact[i]) / ww;
    let mest = s(&|i| wv[i] * est[i]) / ww;
    let cov = s(&|i| wv[i] * (fact[i] - mf) * (est[i] - mest)) / ww;
    let vf = s(&|i| {
        let d = fact[i] - mf;
        wv[i] * (d * d)
    }) / ww;
    let ve = s(&|i| {
        let d = est[i] - mest;
        wv[i] * (d * d)
    }) / ww;
    let r = if vf > 0.0 && ve > 0.0 {
        cov / (vf * ve).sqrt()
    } else {
        f64::NAN
    };
    Metrics {
        me,
        mae,
        rmse,
        msdr,
        r,
    }
}

/// Советы по метрикам: готовые строки на языке показа (`cv_advice`).
pub fn cv_advice(v: &Values<'_>, m: &Metrics) -> Vec<String> {
    let t = |template: &str, a: Vec<Arg>| render(v.tool, template, &a, v.lang);
    let mut tips = Vec::new();
    let rr = if m.rmse != 0.0 && !m.rmse.is_nan() {
        m.rmse
    } else {
        1.0
    };
    let mut good = true;
    if !m.msdr.is_nan() {
        if m.msdr > 1.3 {
            tips.push(t(
                "MSDR заметно больше 1 (%.3g): карта стандартной ошибки занижена. \
                 Умножьте наггет C0 и вклады C на MSDR (радиус и модель не трогайте) \
                 и пересчитайте - сами оценки не изменятся, поправится только \
                 дисперсия кригинга.",
                args![m.msdr],
            ));
            good = false;
        } else if m.msdr < 0.7 {
            tips.push(t(
                "MSDR меньше 1 (%.3g): неопределённость завышена. Разделите наггет \
                 C0 и вклады C на MSDR (радиус и модель не трогайте) и пересчитайте - \
                 оценки не изменятся.",
                args![m.msdr],
            ));
            good = false;
        } else {
            tips.push(t(
                "MSDR близок к 1 (%.3g, полоса от 0.7 до 1.3): масштаб вариограммы \
                 подобран адекватно.",
                args![m.msdr],
            ));
        }
    }
    if m.me.abs() > 0.1 * rr {
        tips.push(t(
            "ME заметно отличается от 0 (%+.3g): возможен систематический сдвиг - \
             проверьте данные и тип кригинга (для простого - заданное среднее).",
            args![m.me],
        ));
        good = false;
    } else {
        tips.push(t("ME близок к 0: систематического смещения нет.", args![]));
    }
    if !m.r.is_nan() {
        if m.r < 0.5 {
            tips.push(t(
                "Низкая корреляция (R=%.2f): модель слабо предсказывает - попробуйте \
                 другой радиус, модель или анизотропию. Либо это предел данных \
                 (короткомасштабная изменчивость, зоны замещения).",
                args![m.r],
            ));
            good = false;
        } else if m.r >= 0.8 {
            tips.push(t(
                "Высокая корреляция (R=%.2f): оценки хорошо согласуются с фактом.",
                args![m.r],
            ));
        }
    }
    let head = if good {
        t(
            "Итог: параметры можно утверждать - перенесите ту же вариограмму и \
             настройки поиска в «2D Kriging».",
            args![],
        )
    } else {
        t(
            "Итог: параметры стоит подправить (см. ниже) и повторить кросс-валидацию \
             перед финальным кригингом.",
            args![],
        )
    };
    tips.insert(0, head);
    tips
}

/// Веса декластеризации по значениям поля (`read_weights`): `None` с
/// предупреждением, если они пустые, неположительные или не числа.
pub fn read_weights(report: &mut Report, ids: &[J], n: usize) -> Option<Vec<f64>> {
    let mut wv = Vec::with_capacity(ids.len());
    for w in ids {
        let x = match w {
            J::Null => f64::NAN,
            J::Bool(b) => f64::from(u8::from(*b)),
            J::Number(x) => x.as_f64().unwrap_or(f64::NAN),
            J::String(s) => match s.trim().parse() {
                Ok(x) => x,
                Err(_) => {
                    report.warn(
                        "Не удалось прочитать поле весов - веса игнорируются.",
                        args![],
                    );
                    return None;
                }
            },
            _ => f64::NAN,
        };
        wv.push(x);
    }
    if wv.len() == n && wv.iter().all(|x| x.is_finite() && *x > 0.0) {
        return Some(wv);
    }
    report.warn(
        "Поле весов содержит пустые или неположительные значения - веса игнорируются.",
        args![],
    );
    None
}
