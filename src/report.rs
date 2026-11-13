// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Отчёт расчёта и отказ расчёта, как `Report` и `ToolError` в
//! `toolspec.py` модуля. Строка хранится шаблоном с аргументами и
//! переводится при показе: шаблон и строковые аргументы ищутся в
//! `messages` описания инструмента.

use rustgis_kit::Lang;
use rustgis_kit::catalog::Tool;

use crate::pyfmt::{self, Arg};

/// Строка отчёта: уровень (`info`, `warn`), шаблон, аргументы.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub level: &'static str,
    pub template: String,
    pub args: Vec<Arg>,
}

impl Line {
    /// Строка на русском, как её пишет модуль в `tool_runs.json`.
    pub fn text(&self) -> String {
        pyfmt::format(&self.template, &self.args)
    }

    /// Строка на языке показа.
    pub fn show(&self, tool: &Tool, lang: Lang) -> String {
        render(tool, &self.template, &self.args, lang)
    }
}

/// Перевод шаблона и строковых аргументов, затем подстановка.
pub fn render(tool: &Tool, template: &str, args: &[Arg], lang: Lang) -> String {
    let args: Vec<Arg> = args
        .iter()
        .map(|a| match a {
            Arg::Text(t) => Arg::Text(tool.translate(t, lang).to_string()),
            other => other.clone(),
        })
        .collect();
    pyfmt::format(tool.translate(template, lang), &args)
}

/// Отчёт расчёта.
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<Line>,
}

impl Report {
    pub fn info(&mut self, template: &str, args: Vec<Arg>) {
        self.lines.push(Line {
            level: "info",
            template: template.to_string(),
            args,
        });
    }

    pub fn warn(&mut self, template: &str, args: Vec<Arg>) {
        self.lines.push(Line {
            level: "warn",
            template: template.to_string(),
            args,
        });
    }
}

/// Расчёт невозможен: шаблон и аргументы для перевода.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub template: String,
    pub args: Vec<Arg>,
}

impl ToolError {
    pub fn new(template: &str, args: Vec<Arg>) -> Self {
        Self {
            template: template.to_string(),
            args,
        }
    }

    /// Текст на русском, как в `tool_runs.json`.
    pub fn text(&self) -> String {
        pyfmt::format(&self.template, &self.args)
    }
}

/// Аргументы шаблона списком: `args![n, x, "текст"]`.
#[macro_export]
macro_rules! args {
    ($($a:expr),* $(,)?) => { vec![$($crate::pyfmt::Arg::from($a)),*] };
}
