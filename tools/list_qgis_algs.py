import re, json, sys

path = r'C:\Dev\qgis-isoliner\grid_isolines\algorithms.py'
text = open(path, 'r', encoding='utf-8').read()

# extract group variable values at module level
group_vars = {}
for m in re.finditer(r'^(GROUP[A-Z0-9_]*)\s*=\s*_tr\("(.*?)"\)', text, re.M):
    group_vars[m.group(1)] = m.group(2)

classes = re.findall(r'class (\w+)\(IsolinerAlgorithm\):', text)
items = []
for cls in classes:
    start = text.find(f'class {cls}(IsolinerAlgorithm):')
    block = text[start:]
    nxt = block.find('\nclass ', 1)
    if nxt != -1:
        block = block[:nxt]
    name_match = re.search(r'def displayName\(self\):\s*return self\.tr\("(.*?)"\)', block)
    group_match = re.search(r'def group\(self\):\s*return self\.tr\((\w+)\)', block)
    gv = group_match.group(1) if group_match else None
    items.append({
        'class': cls,
        'name': name_match.group(1) if name_match else None,
        'group_var': gv,
        'group': group_vars.get(gv),
    })

out = r'C:\Dev\isoliner-rustgis\tools\qgis_algs.json'
with open(out, 'w', encoding='utf-8') as f:
    json.dump(items, f, ensure_ascii=False, indent=2)
