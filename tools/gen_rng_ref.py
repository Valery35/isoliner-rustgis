"""Эталоны default_rng NumPy для tests/data/npy_rng.json.

Запуск Python из QGIS:
    python -I -X utf8 tools/gen_rng_ref.py tests/data/npy_rng.json
"""
import json
import sys

import numpy as np

out = {"numpy": np.__version__, "cases": []}
for seed in (1, 42, 2**40 + 5, 12345678901234567890123):
    r = np.random.default_rng(seed)
    st = r.bit_generator.state["state"]
    case = {"seed": str(seed), "state": str(st["state"]), "inc": str(st["inc"])}
    r = np.random.default_rng(seed)
    case["random"] = [float(x) for x in r.random(5)]
    case["uniform"] = [float(r.uniform(0.35, 0.65)), float(r.uniform(0, 2 * np.pi))]
    # целые вперемешку с равномерными: буфер половинки 32 бит
    ints = []
    for hi in (3, 5, 2**31, 2**32 + 1, 7, 2**40, 4):
        ints.append(int(r.integers(1, hi)))
        ints.append(float(r.random()))
    case["mixed"] = ints
    case["normal"] = [float(x) for x in r.normal(0.0, 1.0, 6000)]
    case["normal_scaled"] = [float(x) for x in r.normal(5.0, 3.0, 50)]
    out["cases"].append(case)
json.dump(out, open(sys.argv[1], "w"), separators=(",", ":"))
print("ok")
