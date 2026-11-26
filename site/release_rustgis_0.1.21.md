Isoliner для RustGIS 0.1.21 - топография, гидрология и гриды.

- Команда 1.03 «MBA»: гладкая поверхность по точкам мультиуровневыми B-сплайнами.
- Команда 1.04 «Изолинии из растра»: marching squares по шагу или заданным уровням, линия с полем уровня.
- Команда 1.07 «Вариограммная карта»: поверхность γ(hx, hy) по точкам с весами декластеризации; в отчёте азимут, отношение и радиус анизотропии.
- Команда 2.03 «Topo2Raster»: рельеф по горизонталям многогосеточной интерполяцией, с водотоками, бровками и озёрами.
- Команда 2.04 «Заполнить понижения»: priority-flood поднимает замкнутые понижения до низшей точки перелива.
- Команда 2.05 «Сток и аккумуляция»: направления D8 по кодам ArcGIS и накопление в ячейках.
- Команда 2.06 «Речная сеть»: звенья от истоков и слияний с порядком Стралера, длиной и накоплением в устье.
- Команда 2.07 «Бассейны и водоразделы»: номер бассейна в каждой ячейке.
- Команда 2.09 «Вершины и ямы»: локальные максимумы и минимумы с минимальной выраженностью, два точечных слоя.
- Команда 2.12 «Невязка горизонталей против ЦМР»: среднее, СКО, RMSE, медиана, максимум и доля за половиной сечения, отдельно по съёмочным и контрольным горизонталям.
- Команда 2.13 «Диагностика террасинга»: профильная кривизна и притяжение отметок к уровням горизонталей.
- Команда 2.14 «Снятие ступеней»: сглаживание ЦМР с удержанием каждой ячейки в доле сечения.
- Команда 2.15 «Отчёт по створу»: морфометрия водосбора по точке створа - площадь, длина водотока, средний уклон, высоты, периметр и время добегания.
- Команда 2.16 «Водосбор линии»: водосборы всех вершин линии объединяются в один.
- Команда 2.17 «Отчёт по водосбору»: площадь, число ячеек и высоты по каждому бассейну.
- Команда 2.18 «Насыпи и выемки»: разность поверхностей и объёмы насыпи, выемки и сальдо.
- Команда 2.19 «Кандидаты бровок»: точки перелома уклона с типом, высотой и контрастом.
- Команда 2.20 «Бровки и подошвы»: пары бровка-подошва линиями с общим полем связи, одиночные линии - в слой-сирота.
- Команда 2.21 «Демо-карьер»: синтетическая ЦМР с карьером и крутыми стенками для проверки инструментов.
- Команда 2.22 «Снятие высот»: отметка ЦМР в поле z каждой точки.
- Команда 2.23 «Линии стока»: пути D8 от стартовых точек до края или ямы.
- Команда 2.24 «Снятие мостов»: ячейки выше окрестности заменяются уровнем вокруг.
- Все растровые результаты - GeoTIFF float32 в системе координат слоя, nodata -9999.
- Инструменты собраны во вкладке «Isoliner» по группам QGIS: 13 быстрых инструментов и 30 команд.
- Нужен RustGIS 0.8.0 и новее, Windows.

## Установка

1. Скачать `isoliner-0.1.21.rgplugin`.
2. В RustGIS открыть вкладку «Модули» и нажать «Установить из файла».
3. Выбрать пакет и подтвердить установку. Прежнюю версию пакет заменяет.

---

Isoliner for RustGIS 0.1.21 - topography, hydrology and grids.

- Command 1.03 MBA: a smooth surface from scattered points by multilevel B-splines.
- Command 1.04 Isolines from raster: marching squares at a contour interval or explicit levels, a line layer with the level field.
- Command 1.07 Variogram map: the surface γ(hx, hy) from points with declustering weights; the report gives the anisotropy azimuth, ratio and range.
- Command 2.03 Topo2Raster: relief from contour lines by multigrid interpolation, with optional streams, breaklines and lakes.
- Command 2.04 Fill depressions: priority-flood raises every closed depression to its lowest spill point.
- Command 2.05 Flow and accumulation: D8 directions with ArcGIS codes and per-cell accumulation.
- Command 2.06 River network: links from sources and junctions with Strahler order, length and outlet accumulation.
- Command 2.07 Basins and watersheds: the basin number in every cell.
- Command 2.09 Peaks and pits: local maxima and minima with a minimum prominence, two point layers.
- Command 2.12 Contour residuals: mean, standard deviation, RMSE, median, maximum and the share beyond half the contour interval, separately for building and validation contours.
- Command 2.13 Terracing check: profile curvature and the attraction of elevations to contour levels.
- Command 2.14 Terrace smooth: smooths a DEM while keeping every cell within a fraction of the contour interval.
- Command 2.15 Gauge report: catchment morphometry from a gauge point - area, stream length, mean slope, elevations, perimeter and time of concentration.
- Command 2.16 Ditch catchment: the watersheds of all line vertices are merged into one.
- Command 2.17 Catchment report: area, cell count and elevations for every basin.
- Command 2.18 Cut and fill: difference of surfaces and cut, fill and net volumes.
- Command 2.19 Breakline candidates: slope-break points with kind, elevation and contrast.
- Command 2.20 Breakline pairs: brow-to-toe pairs as lines with a common link field, unpaired lines go to an orphans layer.
- Command 2.21 Demo quarry: a synthetic DEM with a pit and steep walls for testing the relief tools.
- Command 2.22 Snap elevations: the DEM elevation written into the z field of every point.
- Command 2.23 Downhill traces: D8 paths from start points to the edge or a pit.
- Command 2.24 Bridge removal: cells above their neighbourhood are replaced by the surrounding level.
- All raster outputs are float32 GeoTIFF in the layer CRS, nodata -9999.
- Tools are grouped in the Isoliner tab by QGIS sections: 13 quick tools and 30 commands.
- Needs RustGIS 0.8.0 or later, Windows.

## Install

1. Download `isoliner-0.1.21.rgplugin`.
2. In RustGIS open the Plugins tab and press Install from file.
3. Choose the package and confirm. The package replaces the previous version.
