// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Согласие `plugin.json` с `fixtures/tools.json` и правила RustGIS.
//! `ISOLINER_MANIFEST=1 cargo test` переписывает разделы `tools`, `tab` и их
//! строки в `plugin.json`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use isoliner_rustgis::product::{HIDDEN, OPTIONS, TOOLS_JSON};
use serde_json::{Value as J, json};

fn path() -> String {
    format!("{}/plugin.json", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn manifest_tools_follow_tools_json() {
    let kit = isoliner_rustgis::product::catalog().unwrap();
    let (mut tools, mut ru) = rustgis_kit::manifest::tools_section(&kit, &OPTIONS);
    isoliner_rustgis::product::apply_toolbox_groups(&mut tools, &mut ru);
    // вкладка ленты собирается крейтом из product::tab::TAB
    let base: J = serde_json::from_slice(&std::fs::read(path()).unwrap()).unwrap();
    let commands: Vec<&str> = base["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let (tab, tab_ru) = rustgis_kit::manifest::tab_section(
        &isoliner_rustgis::product::tab::TAB,
        &kit,
        &OPTIONS,
        &commands,
    )
    .unwrap();
    ru.extend(tab_ru);
    if std::env::var_os("ISOLINER_MANIFEST").is_some() {
        let text = std::fs::read_to_string(path()).unwrap();
        let text = rustgis_kit::manifest::splice_all(&text, &tools, Some(&tab), &ru).unwrap();
        std::fs::write(path(), text).unwrap();
    }
    let m: J = serde_json::from_slice(&std::fs::read(path()).unwrap()).unwrap();
    assert_eq!(
        m["tools"],
        json!(tools),
        "plugin.json устарел: ISOLINER_MANIFEST=1 cargo test"
    );
    assert_eq!(
        m["tab"], tab,
        "вкладка устарела: ISOLINER_MANIFEST=1 cargo test"
    );
    for (k, v) in &ru {
        assert_eq!(&m["i18n"]["ru"][k], v, "нет перевода: {k}");
    }
    // каждый предложенный инструмент есть в описаниях и в манифесте
    assert_eq!(tools.len(), OPTIONS.offered.len());
    for name in OPTIONS.offered {
        assert!(kit.tool(name).is_some(), "{name} нет в tools.json");
    }
    // правила RustGIS: имена, число полей, пункты списков
    for t in &tools {
        let id = t["id"].as_str().unwrap();
        assert!(
            id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        let ps = t["params"].as_array().unwrap();
        // полей может не быть (4.11), больше 32 RustGIS 0.6.0 не примет
        assert!(ps.len() <= 32);
        for p in ps {
            let n = p["name"].as_str().unwrap();
            assert!(n.starts_with(|c: char| c.is_ascii_lowercase()), "{n}");
            assert!(
                n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{n}"
            );
            if p["kind"] == "choice" {
                let ch = p["choices"].as_array().unwrap();
                assert!((1..=64).contains(&ch.len()));
                assert!(ch.contains(&p["default"]));
            }
        }
    }
    // скрытые поля есть в описании модуля, у каждого есть умолчание
    let full = rustgis_kit::Catalog::parse(TOOLS_JSON).unwrap();
    for (name, keys) in HIDDEN {
        let t = full.tool(name).unwrap();
        for k in *keys {
            let p = t.param(k).unwrap_or_else(|| panic!("{name}: нет поля {k}"));
            assert!(p.default.is_some(), "{name}.{k} без умолчания");
        }
    }
    check_tab(&m, &kit);
    // версия манифеста - версия крейта
    assert_eq!(m["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(m["id"], "isoliner");
}

/// Значки, которые RustGIS 0.6.0 знает (`crates/ui-egui/src/icons.rs`);
/// неизвестное имя рисуется куском пазла.
const ICONS: &[&str] = &[
    "catalog",
    "about",
    "chart",
    "point_layer",
    "relief",
    "table",
    "measure",
];

/// Группа вкладки по первой цифре кода команды/инструмента модуля.
fn group_of(code: &str) -> &'static str {
    match code.split('.').next().unwrap_or("") {
        "1" => "1. Grid and isolines",
        "2" => "2. Topography",
        "3" => "4. Additional analysis",
        "9" => "10. Subsidence",
        _ => "?",
    }
}

/// Записи меню и группы: (группа, запись), меню разворачиваются.
fn entries<'a>(group: &str, items: &'a [J], out: &mut Vec<(String, &'a J)>, depth: usize) {
    assert!(depth <= 4, "меню глубже четырёх уровней");
    for it in items {
        if let Some(menu) = it["menu"].as_str() {
            let sub = it["items"].as_array().unwrap();
            assert!((1..=32).contains(&sub.len()), "меню {menu}: 1-32 записи");
            out.push((group.to_string(), it));
            entries(group, sub, out, depth + 1);
        } else {
            out.push((group.to_string(), it));
        }
    }
}

/// Вкладка ленты: команды в своих группах, переводы есть, значки известные,
/// пределы RustGIS соблюдены.
fn check_tab(m: &J, _kit: &rustgis_kit::Catalog) {
    let tab = &m["tab"];
    let ru = &m["i18n"]["ru"];
    let label = tab["label"].as_str().unwrap();
    assert!(!["Home", "Data", "Style", "Analyze", "Plugins"].contains(&label));
    let groups = tab["groups"].as_array().unwrap();
    assert!((1..=8).contains(&groups.len()));
    let commands: Vec<&str> = m["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let mut all = Vec::new();
    for g in groups {
        let gl = g["label"].as_str().unwrap();
        assert!(gl.chars().count() <= 32);
        assert!(ru[gl].is_string(), "нет перевода группы {gl}");
        let items = g["items"].as_array().unwrap();
        assert!((1..=16).contains(&items.len()));
        entries(gl, items, &mut all, 1);
    }
    for (group, it) in &all {
        if let Some(menu) = it["menu"].as_str() {
            assert!(ru[menu].is_string(), "нет перевода меню {menu}");
            if let Some(h) = it["help"].as_str() {
                assert!(ru[h].is_string(), "нет перевода подсказки {h}");
            }
            let icon = it["icon"].as_str().unwrap_or("");
            assert!(ICONS.contains(&icon), "значок {icon} неизвестен RustGIS");
            continue;
        }
        let id = it.as_str().unwrap();
        if let Some(tool) = id.strip_prefix("tool:") {
            let t = _kit.by_rg_id(tool).unwrap_or_else(|| panic!("нет инструмента {tool}"));
            assert_eq!(group_of(&t.code), group, "{tool} не в своей группе");
        } else {
            assert!(commands.contains(&id), "нет команды {id}");
        }
    }
    for c in m["commands"].as_array().unwrap() {
        let icon = c["icon"].as_str().unwrap_or("");
        assert!(
            ICONS.contains(&icon),
            "значок команды {icon} неизвестен RustGIS"
        );
    }
}
