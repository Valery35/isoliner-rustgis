// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Согласие `plugin.json` с `fixtures/tools.json` и правила RustGIS.
//! `ISOLINER_MANIFEST=1 cargo test` переписывает раздел `tools` и его
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
    let (tools, ru) = rustgis_kit::manifest::tools_section(&kit, &OPTIONS);
    if std::env::var_os("ISOLINER_MANIFEST").is_some() {
        let text = std::fs::read_to_string(path()).unwrap();
        let text = rustgis_kit::manifest::splice(&text, &tools, &ru).unwrap();
        std::fs::write(path(), text).unwrap();
    }
    let m: J = serde_json::from_slice(&std::fs::read(path()).unwrap()).unwrap();
    assert_eq!(
        m["tools"],
        json!(tools),
        "plugin.json устарел: ISOLINER_MANIFEST=1 cargo test"
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
        assert!(!ps.is_empty() && ps.len() <= 32);
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
    // версия манифеста - версия крейта
    assert_eq!(m["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(m["id"], "isoliner");
}
