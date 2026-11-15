// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Запрос RustGIS одним запуском, как его шлёт RustGIS 0.6.0: слой точек
//! в градусах без Z, значение из поля. Ответ - слой с полем `wt` и
//! журнал на языке запроса.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use isoliner_rustgis::product::Isoliner;
use serde_json::{Value as J, json};

/// Сеть 6 на 6 через 0.001 градуса и плотное скопление богатых проб.
fn layer() -> J {
    let mut feats = Vec::new();
    for i in 0..6 {
        for j in 0..6 {
            let (lon, lat) = (56.0 + f64::from(i) * 0.001, 58.0 + f64::from(j) * 0.001);
            feats.push(json!({"type": "Feature", "properties": {"kcl": 20.0},
                "geometry": {"type": "Point", "coordinates": [lon, lat]}}));
        }
    }
    for k in 0..12 {
        let (lon, lat) = (
            56.0025 + f64::from(k % 4) * 0.00003,
            58.0025 + f64::from(k / 4) * 0.00003,
        );
        feats.push(json!({"type": "Feature", "properties": {"kcl": 30.0},
            "geometry": {"type": "Point", "coordinates": [lon, lat]}}));
    }
    json!({"type": "FeatureCollection", "features": feats})
}

fn ask(lang: &str, params: J) -> J {
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "declustering", "lang": lang,
        "params": params, "layers": {"Скважины": layer()}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn declustering_through_rustgis() {
    let reply = ask("ru", json!({"input": "Скважины", "zfield": "kcl"}));
    let layer = &reply["ok"]["layers"][0];
    assert_eq!(layer["name"], "Точки с весами декластеризации", "{reply}");
    let feats = layer["geojson"]["features"].as_array().unwrap();
    assert_eq!(feats.len(), 48);
    let wt: Vec<f64> = feats
        .iter()
        .map(|f| f["properties"]["wt"].as_f64().unwrap())
        .collect();
    // веса в слое округлены до четырёх значащих цифр, как у модуля
    // (до половины единицы четвёртого знака на точку, 48 точек)
    assert!((wt.iter().sum::<f64>() - 48.0).abs() < 0.03);
    // скопление легче точки сети
    let rich = feats
        .iter()
        .position(|f| f["properties"]["kcl"] == 30.0)
        .unwrap();
    let plain = feats
        .iter()
        .position(|f| f["properties"]["kcl"] == 20.0)
        .unwrap();
    assert!(wt[rich] < wt[plain], "{} {}", wt[rich], wt[plain]);
    // точка ложится обратно в градусы
    let c = &feats[plain]["geometry"]["coordinates"];
    assert!((c[0].as_f64().unwrap() - 56.0).abs() < 0.01);
    assert!((c[1].as_f64().unwrap() - 58.0).abs() < 0.01);
    let msgs: Vec<&str> = reply["ok"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(J::as_str)
        .collect();
    assert_eq!(msgs[0], "== Декластеризация ==", "{msgs:?}");
    assert!(
        msgs[1].starts_with("Точек: 48, размер ячейки: "),
        "{msgs:?}"
    );
    assert!(
        msgs[3].starts_with("Декластеризованное среднее: "),
        "{msgs:?}"
    );
}

#[test]
fn english_and_manual_cell() {
    let reply = ask(
        "en",
        json!({"input": "Скважины", "zfield": "kcl", "mode": "Manual size", "cell_size": 0}),
    );
    let msgs: Vec<&str> = reply["ok"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(J::as_str)
        .collect();
    assert!(
        msgs[0].starts_with("Cell size not set - using "),
        "{msgs:?}"
    );
    assert_eq!(msgs[1], "== Declustering ==");
}

#[test]
fn refusal_without_value_field() {
    // слой RustGIS без Z, поле значения не задано - отказ модуля
    let reply = ask("ru", json!({"input": "Скважины"}));
    let e = reply["error"].as_str().unwrap_or_default();
    assert!(e.contains("геометрия слоя без Z"), "{reply}");
}

/// 1.11 по охвату слоя-полигона в градусах: точки профилей ложатся
/// внутрь охвата, подпись пикета на языке запроса, зерно повторяет данные.
#[test]
fn geophys_profiles_through_rustgis() {
    let area = json!({"type": "FeatureCollection", "features": [{"type": "Feature",
        "properties": {"name": "участок"},
        "geometry": {"type": "Polygon", "coordinates": [[[56.0, 58.0], [56.02, 58.0],
            [56.02, 58.01], [56.0, 58.01], [56.0, 58.0]]]}}]});
    let run = |lang: &str, seed: i64| -> J {
        let p = Isoliner::new().unwrap();
        let req = json!({"abi": 1, "kind": "tool", "id": "geophysprofiles", "lang": lang,
            "params": {"extent": "Участок", "seed": seed}, "layers": {"Участок": area}});
        let mut out = Vec::new();
        rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
        serde_json::from_slice(&out).unwrap()
    };
    let a = run("ru", 7);
    let layer = &a["ok"]["layers"][0];
    assert_eq!(layer["name"], "Геофизические профили", "{a}");
    let feats = layer["geojson"]["features"].as_array().unwrap();
    assert!(!feats.is_empty());
    assert_eq!(feats[0]["properties"]["pk"], "ПК0+00");
    for f in feats {
        let c = &f["geometry"]["coordinates"];
        let (lon, lat) = (c[0].as_f64().unwrap(), c[1].as_f64().unwrap());
        assert!(
            (56.0..=56.02).contains(&lon) && (58.0..=58.01).contains(&lat),
            "{c}"
        );
    }
    let msg = a["ok"]["messages"][0].as_str().unwrap_or_default();
    assert!(msg.starts_with("Электроразведка: профилей 4"), "{msg}");
    // то же зерно - те же данные, другой язык - другая подпись пикета
    let b = run("en", 7);
    let fb = &b["ok"]["layers"][0]["geojson"]["features"];
    assert_eq!(
        fb[0]["properties"]["rho_k"],
        feats[0]["properties"]["rho_k"]
    );
    assert_eq!(fb[0]["properties"]["pk"], "PK0+00");
}

/// 4.13: устья точками в градусах, интервалы и пробы таблицами без
/// геометрии, как их отдаёт RustGIS. Содержание - по длине перекрытия.
#[test]
fn bed_grades_through_rustgis() {
    let pt = |id: &str, lon: f64| {
        json!({"type": "Feature", "properties": {"hole_id": id},
        "geometry": {"type": "Point", "coordinates": [lon, 58.0]}})
    };
    let row = |p: J| json!({"type": "Feature", "properties": p, "geometry": null});
    let collars =
        json!({"type": "FeatureCollection", "features": [pt("H1", 56.0), pt("H2", 56.01)]});
    let intervals = json!({"type": "FeatureCollection", "features": [
        row(json!({"hole_id": "H1", "from": 100.0, "to": 104.0, "code": "КрII"})),
        row(json!({"hole_id": "H2", "from": 90.0, "to": 92.0, "code": "КрII"})),
    ]});
    let chem = json!({"type": "FeatureCollection", "features": [
        row(json!({"hole_id": "H1", "from": 100.0, "to": 102.0, "kcl": 20.0})),
        row(json!({"hole_id": "H1", "from": 102.0, "to": 104.0, "kcl": "30,0"})),
    ]});
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "bed-grades-at-collars", "lang": "ru",
        "params": {"collar": "Устья", "interval": "Интервалы", "chem": "Пробы",
                   "components": "kcl", "bed": "КрII"},
        "layers": {"Устья": collars, "Интервалы": intervals, "Пробы": chem}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let feats = reply["ok"]["layers"][0]["geojson"]["features"]
        .as_array()
        .unwrap();
    assert_eq!(feats.len(), 2, "{reply}");
    let h1 = feats
        .iter()
        .find(|f| f["properties"]["hole_id"] == "H1")
        .unwrap();
    assert_eq!(h1["properties"]["kcl"], 25.0);
    assert_eq!(h1["properties"]["cover"], 1.0);
    let h2 = feats
        .iter()
        .find(|f| f["properties"]["hole_id"] == "H2")
        .unwrap();
    assert!(h2["properties"]["kcl"].is_null());
    assert_eq!(h2["properties"]["n_samp"], 0);
}

/// 7.03: линия в градусах переводится в метры до расчёта, поле D у
/// исходного объекта.
#[test]
fn line_dimension_through_rustgis() {
    let zig: Vec<J> = (0..=60)
        .map(|i| {
            let lon = 56.0 + f64::from(i) * 0.0002;
            let lat = 58.0 + if i % 2 == 0 { 0.0 } else { 0.0001 };
            json!([lon, lat])
        })
        .collect();
    let lines = json!({"type": "FeatureCollection", "features": [{"type": "Feature",
        "properties": {"name": "зигзаг"},
        "geometry": {"type": "LineString", "coordinates": zig}}]});
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "line-dimension", "lang": "ru",
        "params": {"lines": "Линии"}, "layers": {"Линии": lines}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let f = &reply["ok"]["layers"][0]["geojson"]["features"][0];
    let d = f["properties"]["D"].as_f64().unwrap();
    assert!((1.0..=2.0).contains(&d), "{reply}");
    assert_eq!(f["properties"]["name"], "зигзаг");
    assert_eq!(f["geometry"]["type"], "LineString");
    assert!(
        reply["ok"]["messages"][0]
            .as_str()
            .unwrap()
            .starts_with("Среднее D по 1 линиям")
    );
}

/// 1.06: таблица вариограммы и таблица модели - выходы без геометрии,
/// RustGIS получает их в `tables`, а не слоями.
#[test]
fn variogram_tables_through_rustgis() {
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "experimental-variogram", "lang": "ru",
        "params": {"input": "Скважины", "zfield": "kcl", "n_lags": 6},
        "layers": {"Скважины": layer()}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let tables = reply["ok"]["tables"].as_array().unwrap();
    assert!(
        reply["ok"]["layers"].as_array().unwrap().is_empty(),
        "{reply}"
    );
    let vario = tables
        .iter()
        .find(|t| t["name"] == "Таблица вариограммы (расстояние, γ, количество пар)")
        .unwrap();
    let rows = vario["geojson"]["features"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert_eq!(rows[0]["properties"]["series"], "все точки");
    assert!(rows[0]["geometry"].is_null());
    let model = tables
        .iter()
        .find(|t| t["name"] == "Модель вариограммы (новая таблица)");
    if let Some(m) = model {
        assert_eq!(m["geojson"]["features"][0]["properties"]["field"], "kcl");
    }
    let msgs = reply["ok"]["messages"].as_array().unwrap();
    assert!(
        msgs[0].as_str().unwrap().starts_with("Точек: 48."),
        "{msgs:?}"
    );
}

/// 1.08: остатки точками на месте скважин, метрики и советы в журнале.
#[test]
fn crossvalidation_through_rustgis() {
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "crossvalidation", "lang": "ru",
        "params": {"input": "Скважины", "zfield": "kcl", "s1_sill": 20.0, "s1_range": 300.0},
        "layers": {"Скважины": layer()}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let feats = reply["ok"]["layers"][0]["geojson"]["features"]
        .as_array()
        .unwrap();
    assert_eq!(feats.len(), 48, "{reply}");
    assert!(feats[0]["properties"]["z_est"].is_number());
    let msgs: Vec<&str> = reply["ok"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(J::as_str)
        .collect();
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("Точек оценено: 48 из 48")),
        "{msgs:?}"
    );
    assert!(msgs.iter().any(|m| m.starts_with("• Итог:")), "{msgs:?}");
}

/// 1.09 обоими методами. У минимальной кривизны скрытые в окне RustGIS
/// настройки решателя берутся по умолчанию модуля.
#[test]
fn method_crossvalidation_through_rustgis() {
    let ask = |method: &str| -> J {
        let p = Isoliner::new().unwrap();
        let req = json!({"abi": 1, "kind": "tool", "id": "method-crossvalidation", "lang": "ru",
            "params": {"input": "Скважины", "zfield": "kcl", "method": method,
                       "n_validate": 10, "seed": 5, "s1_sill": 20.0, "s1_range": 300.0},
            "layers": {"Скважины": layer()}});
        let mut out = Vec::new();
        rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
        serde_json::from_slice(&out).unwrap()
    };
    for method in ["Кригинг", "Минимальная кривизна"] {
        let reply = ask(method);
        let feats = reply["ok"]["layers"][0]["geojson"]["features"].as_array();
        assert!(
            feats.is_some_and(|f| !f.is_empty() && f.len() <= 10),
            "{method}: {reply}"
        );
        let msgs = reply["ok"]["messages"].as_array().unwrap();
        assert!(
            msgs[0].as_str().unwrap().contains("Проверяем 10 из 48"),
            "{method}: {msgs:?}"
        );
    }
}

/// 7.04: полигон с внутренним кольцом меряется по обоим кольцам, поля
/// источника остаются в слое.
#[test]
fn minkowski_through_rustgis() {
    let ring = |c: f64, r: f64| -> Vec<J> {
        (0..=40)
            .map(|k| {
                let a = f64::from(k) * std::f64::consts::TAU / 40.0;
                let rr = r * (1.0 + 0.15 * (5.0 * a).sin());
                json!([56.0 + c + rr * a.cos(), 58.0 + rr * 0.6 * a.sin()])
            })
            .collect()
    };
    let poly = json!({"type": "FeatureCollection", "features": [{"type": "Feature",
        "properties": {"name": "озеро"},
        "geometry": {"type": "Polygon", "coordinates": [ring(0.0, 0.01), ring(0.0, 0.003)]}}]});
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "minkowski-dimension", "lang": "ru",
        "params": {"features": "Контуры"}, "layers": {"Контуры": poly}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let f = &reply["ok"]["layers"][0]["geojson"]["features"][0];
    assert!(f["properties"]["D_mink"].is_number(), "{reply}");
    assert_eq!(f["properties"]["name"], "озеро");
    let msgs = reply["ok"]["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| {
        m.as_str()
            .unwrap()
            .starts_with("Размерность Минковского слоя")
    }));
}

/// 2.08: горизонтали делятся по отметкам на два слоя, поле hold.
#[test]
fn contour_split_through_rustgis() {
    let feats: Vec<J> = (0..10)
        .map(|k| {
            let lat = 58.0 + f64::from(k) * 0.0005;
            json!({"type": "Feature", "properties": {"elev": 100 + 5 * k},
                "geometry": {"type": "LineString", "coordinates": [[56.0, lat], [56.01, lat]]}})
        })
        .collect();
    let lines = json!({"type": "FeatureCollection", "features": feats});
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "contoursplit", "lang": "ru",
        "params": {"input": "Горизонтали", "field": "elev", "every": 4},
        "layers": {"Горизонтали": lines}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let layers = reply["ok"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2, "{reply}");
    let n = |k: usize| layers[k]["geojson"]["features"].as_array().unwrap().len();
    assert_eq!(n(0) + n(1), 10);
    let check = layers
        .iter()
        .find(|l| l["geojson"]["features"][0]["properties"]["hold"] == 1)
        .unwrap();
    assert_eq!(check["geojson"]["features"][0]["properties"]["elev"], 120);
}

/// 3.08: три слоя - точки, линии и полигоны, суммарная масса 1000.
#[test]
fn density_demo_through_rustgis() {
    let area = json!({"type": "FeatureCollection", "features": [{"type": "Feature",
        "properties": {}, "geometry": {"type": "Polygon", "coordinates": [[[56.0, 58.0],
        [56.02, 58.0], [56.02, 58.01], [56.0, 58.01], [56.0, 58.0]]]}}]});
    let p = Isoliner::new().unwrap();
    let req = json!({"abi": 1, "kind": "tool", "id": "densitydemo", "lang": "ru",
        "params": {"extent": "Участок", "seed": 3}, "layers": {"Участок": area}});
    let mut out = Vec::new();
    rustgis_kit::serve(&p, format!("{req}\n").as_bytes(), &mut out).unwrap();
    let reply: J = serde_json::from_slice(&out).unwrap();
    let layers = reply["ok"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3, "{reply}");
    let mut kinds = Vec::new();
    let mut mass = 0.0;
    for l in layers {
        for f in l["geojson"]["features"].as_array().unwrap() {
            kinds.push(f["geometry"]["type"].as_str().unwrap().to_string());
            mass += f["properties"]["mass"].as_f64().unwrap();
        }
    }
    assert!(kinds.iter().any(|k| k.contains("Polygon")), "{kinds:?}");
    assert!(kinds.iter().any(|k| k.contains("LineString")), "{kinds:?}");
    assert!((mass - 1000.0).abs() < 1e-6, "{mass}");
}
