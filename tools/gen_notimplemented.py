import json

with open(r'C:\Dev\isoliner-rustgis\tools\qgis_algs.json', 'r', encoding='utf-8') as f:
    qgis = json.load(f)

with open(r'C:\Dev\isoliner-rustgis\plugin.json', 'r', encoding='utf-8') as f:
    plugin = json.load(f)

commands = [c['id'] for c in plugin['commands']]
tools = [t['id'] for t in plugin['tools']]

mapping = {
    # tools
    'declustering': 'DeclusteringAlgorithm',
    'geophysprofiles': 'GeophysProfilesDemoAlgorithm',
    'experimental-variogram': 'ExperimentalVariogramAlgorithm',
    'crossvalidation': 'CrossValidationAlgorithm',
    'method-crossvalidation': 'MethodCrossValidationAlgorithm',
    'examplewells': 'ExampleWellsAlgorithm',
    'contoursplit': 'ContourSplitAlgorithm',
    'densitydemo': 'DensityDemoAlgorithm',
    'bed-grades-at-collars': 'BedGradesAtCollarsAlgorithm',
    'plast-reference-template': 'PlastReferenceTemplateAlgorithm',
    'line-dimension': 'LineDimensionAlgorithm',
    'minkowski-dimension': 'MinkowskiDimensionAlgorithm',
    'fractal-demo': 'FractalDemoAlgorithm',
    # commands
    'slope-aspect': 'SlopeAspectAlgorithm',
    'minimum-curvature': 'MinCurvatureAlgorithm',
    'kriging-2d': 'Kriging2DAlgorithm',
    'probability-threshold': 'ExceedanceProbabilityAlgorithm',
    'flow-gradient': 'FlowGradientAlgorithm',
    'subsidence-tilt-curvature': 'SubsidenceTiltCurvatureAlgorithm',
    'subsidence-horizontal': 'SubsidenceHorizontalAlgorithm',
    'fill-depressions': 'TopoFillDepressionsAlgorithm',
    'flow-d8': 'FlowD8Algorithm',
    'river-network': 'RiverNetworkAlgorithm',
    'peaks-pits': 'PeaksAlgorithm',
    'basins': 'BasinsAlgorithm',
    'gauge-report': 'GaugeReportAlgorithm',
    'ditch-catchment': 'DitchCatchmentAlgorithm',
    'catchment-stats': 'CatchmentStatsAlgorithm',
    'cut-fill': 'CutFillAlgorithm',
    'breakline-candidates': 'BreaklineCandidatesAlgorithm',
    'topo-demo-pit': 'TopoDemoPitAlgorithm',
    'snap-elevations': 'SnapElevationsAlgorithm',
    'downhill-trace': 'DownhillTraceAlgorithm',
    'bridge-removal': 'BridgeRemovalAlgorithm',
    'terrace-smooth': 'TerraceSmoothAlgorithm',
    'contour-residual': 'ContourResidualAlgorithm',
    'terracing-check': 'TerracingCheckAlgorithm',
    'topo2raster': 'Topo2RasterAlgorithm',
    'breakline-pairs': 'BreaklinePairsAlgorithm',
    'mba-grid': 'MbaGridAlgorithm',
    'raster-to-isolines': 'RasterToIsolinesAlgorithm',
    'variogram-map': 'VariogramMapAlgorithm',
}

implemented = set(mapping.values())
not_implemented = [a for a in qgis if a['name'] and a['class'] not in implemented]

by_group = {}
for a in not_implemented:
    g = a.get('group') or 'Без группы'
    by_group.setdefault(g, []).append(f"{a['name']} ({a['class']})")

order = [
    '1. Грид и изолинии',
    '2. Топография',
    '2. Топография: диагностика и правка',
    '3. Дополнительные инструменты анализа',
    '4. Разрез',
    '5. Геологическая модель (бета)',
    '6. Гидрология',
    '7. Фрактальный анализ',
    '8. Обмен данными',
    '9. Сдвижения',
]

implemented_count = len([c for c in commands if c != 'about']) + len(tools)
total = len(qgis)

lines = []
lines.append('# Isoliner для RustGIS — нереализованные инструменты модуля QGIS')
lines.append('')
lines.append(f'Состояние на версию {plugin["version"]}. Реализовано {implemented_count} инструментов/команд из {total} в модуле QGIS.')
lines.append('')
lines.append('## Нереализованные инструменты по группам')
lines.append('')
for g in order:
    if g in by_group:
        lines.append(f'### {g}')
        lines.append('')
        for n in sorted(by_group[g]):
            lines.append(f'- {n}')
        lines.append('')
if 'Без группы' in by_group:
    lines.append('### Без группы')
    lines.append('')
    for n in sorted(by_group['Без группы']):
        lines.append(f'- {n}')
    lines.append('')
lines.append('## Примечания')
lines.append('')
lines.append('- Растровые инструменты реализуются как команды RustGIS, потому что в версии 0.8.0 инструмент расширения не принимает и не возвращает растры.')
lines.append('- Демо-инструменты с растровым выходом в QGIS здесь пока выдают только векторные слои.')
lines.append('- Номера и названия групп приведены в соответствие с модулем QGIS `grid_isolines/algorithms.py`.')

with open(r'C:\Dev\isoliner-rustgis\doc\NOT_IMPLEMENTED.md', 'w', encoding='utf-8') as f:
    f.write('\n'.join(lines) + '\n')
