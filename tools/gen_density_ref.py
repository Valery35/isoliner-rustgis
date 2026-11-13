"""Эталон density.demo_dataset модуля для tests/data/density_demo.json.

Запуск Python из QGIS:
    python -I -X utf8 tools/gen_density_ref.py C:/Dev/qgis-isoliner tests/data/density_demo.json
"""
import importlib.util
import json
import sys

module_root, out_path = sys.argv[1], sys.argv[2]
spec = importlib.util.spec_from_file_location(
    "density", module_root + "/grid_isolines/density.py")
density = importlib.util.module_from_spec(spec)
spec.loader.exec_module(density)

cases = []
for ext, seed in (((500000.0, 6400000.0, 501000.0, 6400800.0), 1),
                  ((0.0, 0.0, 300.0, 1000.0), 42)):
    d = density.demo_dataset(*ext, seed=seed)
    cases.append({
        "ext": ext, "seed": seed,
        "points": [list(map(float, p)) for p in d["points"]],
        "lines": [{"verts": [list(v) for v in ln["verts"]], "mass": ln["mass"],
                   "half": ln["half"], "from_m": ln["from_m"], "to_m": ln["to_m"]}
                  for ln in d["lines"]],
        "polygons": [{"rings": [[list(v) for v in r] for r in pg["rings"]],
                      "mass": pg["mass"], "dasy": pg["dasy"]}
                     for pg in d["polygons"]],
    })
json.dump({"cases": cases}, open(out_path, "w"), separators=(",", ":"))
print("ok", len(cases))
