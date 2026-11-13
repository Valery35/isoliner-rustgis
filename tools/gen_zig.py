"""Таблицы зиккурата NumPy (ziggurat_constants.h) -> src/ziggurat.rs."""
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
text = open(src, encoding="utf-8").read()


def table(name):
    m = re.search(r"\b%s\[\]\s*=\s*\{(.*?)\};" % re.escape(name), text, re.S)
    if not m:
        raise SystemExit("no " + name)
    items = [s.strip() for s in m.group(1).replace("\n", " ").split(",")]
    items = [s for s in items if s]
    return items


def scalar(name):
    m = re.search(r"\b%s\s*=\s*([^;]+);" % re.escape(name), text)
    if not m:
        raise SystemExit("no " + name)
    return m.group(1).strip()


ki = [re.sub(r"(ULL|UL|U)$", "", s) for s in table("ki_double")]
wi = table("wi_double")
fi = table("fi_double")
assert len(ki) == len(wi) == len(fi) == 256, (len(ki), len(wi), len(fi))
r = scalar("ziggurat_nor_r")
inv_r = scalar("ziggurat_nor_inv_r")


def fl(s):
    s = s.strip()
    # Rust: литерал f64 обязан иметь точку или показатель
    if not re.search(r"[.eE]", s):
        s += ".0"
    return s


out = []
out.append("// Isoliner для RustGIS.")
out.append("// Таблицы зиккурата нормального распределения NumPy 2.4.6,")
out.append("// numpy/random/src/distributions/ziggurat_constants.h")
out.append("// (лицензия NumPy - BSD-3-Clause). Файл собран сценарием, не править.")
out.append("")
out.append("pub const NOR_R: f64 = %s;" % fl(r))
out.append("pub const NOR_INV_R: f64 = %s;" % fl(inv_r))
out.append("")
out.append("pub const KI: [u64; 256] = [")
out += ["    %s," % s for s in ki]
out.append("];")
out.append("")
out.append("#[allow(clippy::excessive_precision, clippy::unreadable_literal)]")
out.append("pub const WI: [f64; 256] = [")
out += ["    %s," % fl(s) for s in wi]
out.append("];")
out.append("")
out.append("#[allow(clippy::excessive_precision, clippy::unreadable_literal)]")
out.append("pub const FI: [f64; 256] = [")
out += ["    %s," % fl(s) for s in fi]
out.append("];")
open(dst, "w", encoding="utf-8", newline="\n").write("\n".join(out) + "\n")
print("ok", len(ki), r, inv_r)
