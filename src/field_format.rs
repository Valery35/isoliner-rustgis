// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Точность чисел в выходных полях, как `grid_isolines/field_format.py`
//! модуля (`round_value`). Слой в RustGIS несёт те же числа, что слой
//! модуля в QGIS.

use crate::pyfmt::{self, Arg};

/// Точность поля: знаков после запятой или значащих цифр.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec {
    Decimals(i32),
    Significant(usize),
}

/// Округлить число. Не конечное возвращается как есть.
pub fn round_value(v: f64, spec: Spec) -> f64 {
    if !v.is_finite() {
        return v;
    }
    let r = match spec {
        // round(value, n) Python: точное десятичное округление половины к чётному
        Spec::Decimals(n) => {
            let n = n.max(0) as usize;
            format!("{v:.n$}").parse().unwrap_or(v)
        }
        Spec::Significant(n) => {
            if v == 0.0 {
                return 0.0;
            }
            pyfmt::format(&format!("%.{n}g"), &[Arg::Float(v)])
                .parse()
                .unwrap_or(v)
        }
    };
    // + 0.0 модуля: минус ноль - в ноль
    r + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_module() {
        assert_eq!(
            round_value(0.544_952_876_984_127_1, Spec::Significant(4)),
            0.545
        );
        assert_eq!(
            round_value(1.416_666_666_666_666_7, Spec::Significant(4)),
            1.417
        );
        assert_eq!(round_value(123_456.7, Spec::Significant(4)), 123_500.0);
        assert_eq!(round_value(2.675, Spec::Decimals(2)), 2.67);
        assert_eq!(
            round_value(-0.0001, Spec::Decimals(2)).to_bits(),
            0.0f64.to_bits()
        );
        assert!(round_value(f64::NAN, Spec::Decimals(2)).is_nan());
    }
}
