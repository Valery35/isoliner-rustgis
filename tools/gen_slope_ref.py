# Isoliner для RustGIS.
# Copyright (C) 2026 ООО «Информ++». Лицензия Apache 2.0.
# SPDX-License-Identifier: Apache-2.0
"""Эталон 2.05 «Уклон и экспозиция» для tests/raster_cmd.rs.

Вырезает кусок ЦМР с пропусками nodata, пишет его GeoTIFF-ом float32
(Deflate, предиктор 3, как выгружает RustGIS) и считает уклон и
экспозицию функцией модуля topo_surface.slope_aspect. Выходы модуля
пишутся сырыми float32 little-endian, nodata -9999.

Запуск питоном QGIS (нужны GDAL и NumPy):
    python-qgis.bat tools/gen_slope_ref.py <ЦМР.tif> <grid_isolines>
"""

import os
import sys

import numpy as np
from osgeo import gdal

gdal.UseExceptions()


def main(dem, module_dir):
    sys.path.insert(0, module_dir)
    import topo_surface

    out = os.path.join(os.path.dirname(__file__), "..", "tests", "data")
    src = gdal.Open(dem)
    # кусок у края данных: в нём есть и nodata, и склоны (первое окно
    # с долей nodata от пятой части до половины)
    w, h = 48, 36
    full = src.GetRasterBand(1).ReadAsArray()
    fnd = src.GetRasterBand(1).GetNoDataValue()
    x0 = y0 = 0
    for y0 in range(0, full.shape[0] - h, 12):
        found = False
        for x0 in range(0, full.shape[1] - w, 12):
            frac = float(np.mean(full[y0:y0 + h, x0:x0 + w] == fnd))
            if 0.2 <= frac <= 0.5:
                found = True
                break
        if found:
            break
    sub = os.path.join(out, "dem_small.tif")
    gdal.Translate(sub, src, srcWin=[x0, y0, w, h], outputType=gdal.GDT_Float32,
                   creationOptions=["COMPRESS=DEFLATE", "PREDICTOR=3"])
    d = gdal.Open(sub)
    b = d.GetRasterBand(1)
    z = b.ReadAsArray().astype(np.float64)
    nd = b.GetNoDataValue()
    mask = (z == nd) if nd is not None else ~np.isfinite(z)
    gt = d.GetGeoTransform()
    cell = 0.5 * (abs(gt[1]) + abs(gt[5]))
    s, a = topo_surface.slope_aspect(z, cell, nodata_mask=mask)
    for name, arr in (("slope", s), ("aspect", a)):
        v = np.where(np.isfinite(arr), arr, -9999.0).astype("<f4")
        v.tofile(os.path.join(out, "dem_small_%s.f32" % name))
    print("cells", z.size, "nodata", int(mask.sum()))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
