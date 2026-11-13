"""Эталон fractal.minkowski_dimension модуля для tests/data/minkowski.json.

Запуск Python из QGIS:
    python -I -X utf8 tools/gen_minkowski_ref.py C:/Dev/qgis-isoliner tests/data/minkowski.json
"""
import importlib.util
import json
import math
import sys

module_root, out_path = sys.argv[1], sys.argv[2]
spec = importlib.util.spec_from_file_location(
    "fractal", module_root + "/grid_isolines/fractal.py")
fractal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fractal)


def koch(p, q, depth):
    if depth == 0:
        return [p]
    (x0, y0), (x1, y1) = p, q
    dx, dy = (x1 - x0) / 3.0, (y1 - y0) / 3.0
    a = (x0 + dx, y0 + dy)
    b = (x0 + 2 * dx, y0 + 2 * dy)
    c = (a[0] + dx * 0.5 - dy * math.sqrt(3) / 2, a[1] + dy * 0.5 + dx * math.sqrt(3) / 2)
    pts = []
    for s, e in ((p, a), (a, c), (c, b), (b, q)):
        pts += koch(s, e, depth - 1)
    return pts


line = koch((500000.0, 6400000.0), (501000.0, 6400000.0), 4) + [(501000.0, 6400000.0)]
zig = [(500000.0 + 10.0 * i, 6401000.0 + (7.0 if i % 2 else 0.0)) for i in range(60)]
cases = []
for name, lines, kw in (
        ("koch", [line], {}),
        ("koch_zig", [line, zig], {"offsets": 5, "seed": 3}),
        ("vertices", [zig], {"densify": 0.0}),
):
    d, r2, sizes, counts = fractal.minkowski_dimension(lines, **kw)
    cases.append({"name": name, "lines": [[list(p) for p in pl] for pl in lines],
                  "kw": kw, "d": d, "r2": r2, "sizes": list(map(float, sizes)),
                  "counts": [int(c) for c in counts]})
json.dump({"cases": cases}, open(out_path, "w"), separators=(",", ":"))
print("ok", [(c["name"], c["d"]) for c in cases])
