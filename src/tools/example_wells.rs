// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! 1.10 Пример скважин (демо), порт `grid_isolines/tools/example_wells.py`.
//!
//! Скважины в охвате слоя с кровлей, мощностью и содержанием X с
//! пространственной структурой, по флажкам минеральный тип, напор и
//! фильтрационные свойства, линия разлома со сдвигом значений. Растра
//! дрейфа нет: растров у расширений RustGIS нет. Генератор NumPy с зерном
//! SEED (0 - случайно), порядок обращений тот же.

use serde_json::{Value as J, json};

use crate::args;
use crate::demo_fields::{demo_field, demo_sample, demo_values};
use crate::field_format::{plan_for, row};
use crate::npy_rng::Rng;
use crate::numpy;
use crate::report::{Report, ToolError};
use crate::tools::Values;
use crate::tools::density_demo::layer;
use crate::tools::geophys_profiles::bbox;

/// Подписи полей выходов (`FIELD_LABELS` класса).
pub const FIELD_LABELS: &[(&str, &str)] = &[
    ("well", "Скважина"),
    ("roof", "Отметка кровли, м"),
    ("thick", "Мощность, м"),
    ("X", "Содержание X, %"),
    ("head", "Напор, м"),
    ("K", "Коэффициент фильтрации K, м/сут"),
    ("T", "Водопроводимость T, м²/сут"),
    ("mintype", "Минеральный тип"),
    ("dz", "Значение для дрейфа dz"),
    ("name", "Название"),
    ("throw", "Амплитуда разлома"),
];

const G: usize = 200;

/// `int(round(x))` Python.
fn iround(x: f64) -> usize {
    x.round_ties_even().max(0.0) as usize
}

pub fn run(v: &Values<'_>, report: &mut Report) -> Result<J, ToolError> {
    let (xmin, ymin, xmax, ymax) = bbox(v.items("EXTENT"))
        .filter(|e| e.2 > e.0 && e.3 > e.1)
        .ok_or_else(|| ToolError::new("Не задана корректная область (экстент).", args![]))?;
    let n = v.integer("N_POINTS").max(0) as usize;
    let (vmin, vmax) = (v.number("VMIN"), v.number("VMAX"));
    if vmax <= vmin || vmax.is_nan() || vmin.is_nan() {
        return Err(ToolError::new(
            "Максимум значения должен быть больше минимума.",
            args![],
        ));
    }
    let smooth = v.number("SMOOTH");
    let nug = v.number("NUGGET_FRAC");
    let mut rng = Rng::from_module_seed(v.integer("SEED"));
    let ext = (xmin, xmax, ymin, ymax);
    let (roof_min, roof_max) = (v.number("ROOF_MIN"), v.number("ROOF_MAX"));
    let (thick_min, thick_max) = (v.number("THICK_MIN"), v.number("THICK_MAX"));

    let w = iround(smooth * G as f64).max(1);
    let xs: Vec<f64> = (0..n).map(|_| rng.uniform(xmin, xmax)).collect();
    let ys: Vec<f64> = (0..n).map(|_| rng.uniform(ymin, ymax)).collect();
    let mut vals_x = demo_values(&mut rng, G, w, &xs, &ys, ext, vmin, vmax, nug);
    let roof = demo_values(
        &mut rng,
        G,
        w,
        &xs,
        &ys,
        ext,
        roof_min.min(roof_max),
        roof_min.max(roof_max),
        0.05,
    );
    let thick = demo_values(
        &mut rng,
        G,
        w,
        &xs,
        &ys,
        ext,
        thick_min.min(thick_max),
        thick_min.max(thick_max),
        (nug * 0.6).min(0.5),
    );

    let tr = |s: &str| v.tool.translate(s, v.lang).to_string();
    let want_mt = v.flag("MINTYPE");
    let mt: Option<Vec<String>> = want_mt.then(|| {
        demo_values(&mut rng, G, w, &xs, &ys, ext, 0.0, 1.0, 0.12)
            .iter()
            .map(|r| {
                if *r > 0.72 {
                    tr("Каменная соль замещения")
                } else if *r > 0.55 {
                    tr("Частичное замещение кс")
                } else {
                    tr("Сильвинит")
                }
            })
            .collect()
    });

    let want_hydro = v.flag("HYDRO");
    let want_head = v.flag("HEAD") || want_hydro;
    let head: Option<Vec<f64>> = want_head.then(|| {
        let ang = rng.uniform(0.0, 2.0 * std::f64::consts::PI);
        let (ux, uy) = (ang.cos(), ang.sin());
        let proj: Vec<f64> = xs
            .iter()
            .zip(&ys)
            .map(|(x, y)| (x - xmin) * ux + (y - ymin) * uy)
            .collect();
        let lo = proj.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = proj.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
        let local = demo_values(&mut rng, G, w, &xs, &ys, ext, -2.0, 2.0, 0.05);
        proj.iter()
            .zip(&local)
            .map(|(p, l)| 100.0 + 20.0 * (1.0 - (p - lo) / span) + l)
            .collect()
    });

    let hydro: Option<(Vec<f64>, Vec<f64>)> = want_hydro.then(|| {
        let wk = iround((smooth * 1.2).clamp(0.06, 0.5) * G as f64).max(1);
        let kf = demo_field(&mut rng, G, wk);
        let z = demo_sample(&kf, &xs, &ys, ext);
        let m = numpy::mean(&z);
        let s = {
            let x = numpy::var(&z).sqrt();
            if x == 0.0 { 1.0 } else { x }
        };
        let k: Vec<f64> = z
            .iter()
            .map(|v| (-1.8 + 1.3 * ((v - m) / s).clamp(-2.5, 2.5)).exp())
            .collect();
        let t = k.iter().zip(&thick).map(|(k, h)| k * h.max(0.1)).collect();
        (k, t)
    });

    let mut fields: Vec<(&str, &str)> = vec![
        ("well", "string"),
        ("roof", "double"),
        ("thick", "double"),
        ("X", "double"),
    ];
    if want_head {
        fields.push(("head", "double"));
    }
    if want_hydro {
        fields.extend([("K", "double"), ("T", "double")]);
    }
    if want_mt {
        fields.push(("mintype", "string"));
    }

    // разлом поперёк площади, не доходящий до краёв, сдвиг по одну сторону
    let throw = {
        let t = v.number("THROW");
        if t.is_nan() { 0.0 } else { t }
    };
    let fx = xmin + 0.5 * (xmax - xmin);
    let fy0 = ymin + 0.12 * (ymax - ymin);
    let fy1 = ymin + 0.72 * (ymax - ymin);
    let skip = v.input_fields();
    let mut out = serde_json::Map::new();
    if v.values["OUTPUT_FAULT"] != J::Bool(false) {
        let ff = [("name", "string"), ("throw", "double")];
        let names: Vec<&str> = ff.iter().map(|f| f.0).collect();
        let plan = plan_for(&names, FIELD_LABELS, &[], &skip);
        let (values, raw) = row(
            &names,
            vec![json!(tr("Разлом (демо)")), json!(throw)],
            &plan,
        );
        let feat = json!({
            "geometry": {"points": [[fx, fy0, null], [fx, fy1, null]]},
            "values": values,
            "raw": raw,
        });
        out.insert("OUTPUT_FAULT".into(), layer("line", &ff, vec![feat]));
    }
    if throw.abs() > 1e-12 {
        for i in 0..n {
            if xs[i] > fx && ys[i] >= fy0 && ys[i] <= fy1 {
                vals_x[i] += throw;
            }
        }
        report.info(
            "Разлом с амплитудой %.3g: значения сдвинуты по одну сторону от линии. \
             Линия не доходит до краёв площади, и выше её конца поверхность обязана смыкаться.",
            args![throw],
        );
    }

    let names: Vec<&str> = fields.iter().map(|f| f.0).collect();
    let plan = plan_for(&names, FIELD_LABELS, &[], &skip);
    let wells: Vec<J> = (0..n)
        .map(|i| {
            let mut r = vec![
                json!(format!("SK-{:04}", i + 1)),
                json!(roof[i]),
                json!(thick[i]),
                json!(vals_x[i]),
            ];
            if let Some(h) = &head {
                r.push(json!(h[i]));
            }
            if let Some((k, t)) = &hydro {
                r.extend([json!(k[i]), json!(t[i])]);
            }
            if let Some(m) = &mt {
                r.push(json!(m[i]));
            }
            let (values, raw) = row(&names, r, &plan);
            json!({"geometry": {"xyz": [xs[i], ys[i], null]}, "values": values, "raw": raw})
        })
        .collect();
    out.insert("OUTPUT".into(), layer("point", &fields, wells));

    let rng_m = 2.0 * smooth * (xmax - xmin).max(ymax - ymin);
    let var = numpy::var(&vals_x);
    report.info(
        "Сгенерировано скважин: %d. Поля: кровля (roof), мощность (thick), содержание X. \
         Дисперсия X ≈ %.4g.",
        args![n, var],
    );
    report.info(
        "Стартовая вариограмма для X (кригинг/кросс-валидация): суммарный силл ≈ %.4g, \
         наггет C0 ≈ %.4g, радиус ≈ %.4g (в единицах координат). Уточните наггет по \
         кросс-валидации до MSDR ≈ 1.",
        args![var, nug * var, rng_m],
    );
    if let Some(m) = &mt {
        // счёт в порядке первого появления, как Counter
        let mut counts: Vec<(String, usize)> = Vec::new();
        for s in m {
            match counts.iter_mut().find(|(k, _)| k == s) {
                Some(c) => c.1 += 1,
                None => counts.push((s.clone(), 1)),
            }
        }
        let txt: Vec<String> = counts.iter().map(|(k, c)| format!("{k}={c}")).collect();
        report.info("%s%s", args!["Поле mintype (демо): ", txt.join(", ")]);
    }
    if want_head {
        report.info(
            "Поле напора (head): региональный уклон + локальная вариация. Кригуйте head, \
             затем подайте растр в «Гидравлический градиент и направление потока».",
            args![],
        );
    }
    if let Some((k, _)) = &hydro {
        let lo = k.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = k.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        report.info(
            "Поля K и T (демо): K лог-нормально (K ≈ %.4g…%.4g м/сут), T = K·мощность. \
             Для удельного расхода создайте калькулятором поля ln(K) и ln(T), кригуйте их, \
             а при подаче в «Удельный расход (Дарси)» включите галку «Растры заданы как ln». \
             Напор (head) кригуйте как обычно.",
            args![lo, hi],
        );
    }
    Ok(J::Object(out))
}
