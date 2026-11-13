// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Запуск: `isoliner-rustgis rustgis-plugin` - так его зовёт RustGIS по
//! `plugin.json`, сессия или одна команда решает крейт.
//! `isoliner-rustgis manifest` печатает `plugin.json` с разделом `tools`.

use std::process::ExitCode;

use isoliner_rustgis::product::{Isoliner, MANIFEST, manifest};

fn main() -> ExitCode {
    let product = match Isoliner::new() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tools.json: {e}");
            return ExitCode::FAILURE;
        }
    };
    match std::env::args().nth(1).as_deref() {
        Some("rustgis-plugin") => rustgis_kit::serve_stdio(&product),
        Some("manifest") => match manifest(&product.catalog, MANIFEST) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("Isoliner для RustGIS. Запуск: isoliner-rustgis rustgis-plugin | manifest");
            ExitCode::from(2)
        }
    }
}
