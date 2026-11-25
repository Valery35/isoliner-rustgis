// Isoliner для RustGIS.
// Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Гидрогеология: градиент напора и направление потока.
//!
//! Порт `grid_isolines/hydro.py`. Строка 0 растра — север, индекс строки
//! растёт на юг, поэтому производная «на север» берётся с обратным знаком.

const EPS: f64 = 1e-12;

/// Градиент напора. Возвращает (модуль |∇h|, азимут потока в градусах).
/// `z` — строки с севера, `nodata` — пропуск. Ячейки на краю и с соседом
/// nodata получают nodata. На плоских участках азимут = nodata.
pub fn head_gradient(z: &[f64], width: usize, height: usize, cellx: f64, celly: f64, nodata: f64) -> (Vec<f64>, Vec<f64>) {
    let n = width * height;
    let mut mag = vec![nodata; n];
    let mut az = vec![nodata; n];
    let is_valid = |v: f64| v.is_finite() && v != nodata;
    for row in 1..height.saturating_sub(1) {
        for col in 1..width.saturating_sub(1) {
            let idx = row * width + col;
            let v = z[idx];
            if !is_valid(v) {
                continue;
            }
            let west = z[row * width + (col - 1)];
            let east = z[row * width + (col + 1)];
            let north = z[(row - 1) * width + col];
            let south = z[(row + 1) * width + col];
            if !is_valid(west) || !is_valid(east) || !is_valid(north) || !is_valid(south) {
                continue;
            }
            let gx = (east - west) / (2.0 * cellx);
            let gy = -(south - north) / (2.0 * celly);
            let m = (gx * gx + gy * gy).sqrt();
            mag[idx] = m;
            if m > EPS {
                let a = (-gx).atan2(-gy).to_degrees();
                az[idx] = if a < 0.0 { a + 360.0 } else { a };
            }
        }
    }
    (mag, az)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_gradient_is_constant() {
        // h = 2x + 3y + 5 (x, y в метрах).
        // gx = dh/dx = 2, gy = dh/dy = 3, mag = sqrt(13) ≈ 3.6056.
        // az = atan2(-gx, -gy) = atan2(-2, -3) ≈ -146.3 -> 213.7°.
        let width = 12;
        let height = 10;
        let cell = 10.0;
        let mut z = vec![-9999.0; width * height];
        for row in 0..height {
            for col in 0..width {
                let x = col as f64 * cell;
                let y = (height - 1 - row) as f64 * cell;
                z[row * width + col] = 2.0 * x + 3.0 * y + 5.0;
            }
        }
        let (mag, az) = head_gradient(&z, width, height, cell, cell, -9999.0);
        // берём внутреннюю ячейку
        let idx = 5 * width + 5;
        assert!((mag[idx] - 3.6055512755).abs() < 1e-6, "{}", mag[idx]);
        assert!((az[idx] - 213.6900675).abs() < 1e-3, "{}", az[idx]);
    }
}
