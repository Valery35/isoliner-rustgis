Isoliner для RustGIS 0.1.5 - 2D кригинг (точки → растр).

- Новая команда 1.01 «2D Kriging» в группе «Топография». В окне выбирается точечный слой и числовое поле значения, в проект ложится растр GeoTIFF float32 на заданной сетке. Расчёт ведётся ординарным или простым кригингом с одной структурой вариограммы (сферическая, экспоненциальная, гауссова или степенная), как в модуле QGIS Isoliner.
- В окне задаются: тип кригинга, среднее для простого кригинга, радиус поиска, минимальное и максимальное число соседей, наггет, модель вариограммы, вклад, радиус корреляции, азимут и анизотропия. Размер ячейки и охват сетки настраиваются, охват можно взять от точек или от слоя.
- Дополнительно: снятие полиномиального тренда степени 1 или 2, логарифмирование значений (для лог-нормальных величин), выходной растр стандартной ошибки кригинга, отсев или срезка ураганных проб по процентилю и явным границам.
- Расчёт идёт в системе координат слоя: команда запрашивает точки у RustGIS в метрах слоя, а не в градусах.
- Нужен RustGIS 0.7.0 и новее, Windows.

## Установка

1. Скачать `isoliner-0.1.5.rgplugin`.
2. В RustGIS открыть вкладку «Модули» и нажать «Установить из файла».
3. Выбрать пакет и подтвердить установку. Прежнюю версию пакет заменяет.

---

Isoliner for RustGIS 0.1.5 - 2D kriging (points to raster).

- A new command, 1.01 2D Kriging, in the Topography group. Its window takes a point layer and a numeric value field, and adds a float32 GeoTIFF raster on the computed grid to the project. The estimate is computed by ordinary or simple kriging with one variogram structure (spherical, exponential, Gaussian or power), as in the QGIS Isoliner plugin.
- The dialog exposes: kriging type, mean for simple kriging, search radius, minimum and maximum number of neighbours, nugget, variogram model, sill/contribution, correlation range, azimuth and anisotropy. Cell size and grid extent are configurable; extent may come from the points or from a layer.
- Optional features: polynomial detrend of degree 1 or 2, log-transform of values (for log-normal variables), an output raster of kriging standard error, and outlier clipping/capping by percentile or explicit bounds.
- The computation uses the layer's own coordinate system: the command asks RustGIS for features in the layer CRS, not in degrees.
- Needs RustGIS 0.7.0 or later, Windows.

## Install

1. Download `isoliner-0.1.5.rgplugin`.
2. In RustGIS open the Plugins tab and press Install from file.
3. Choose the package and confirm. The package replaces the previous version.
