// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Isoliner для RustGIS - расширение-процесс RustGIS на крейте
//! `rustgis-kit`. Инструменты модуля QGIS Isoliner считаются здесь
//! по тем же описаниям (`fixtures/tools.json`) и сверяются с прогонами
//! модуля (`fixtures/tool_runs.json`).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod declus;
pub mod field_format;
pub mod numpy;
pub mod points;
pub mod product;
pub mod pyfmt;
pub mod report;
pub mod tools;
