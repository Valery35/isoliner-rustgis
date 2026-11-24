Isoliner для RustGIS 0.1.4 - интерполяция методом минимальной кривизны.

- Новая команда 1.02 «Минимальная кривизна» в группе «Топография». В окне выбирается точечный слой и числовое поле значения, в проект ложится растр GeoTIFF float32 той же сетки. Поверхность строится бигармоническим уравнением с натяжением, как в модуле QGIS Isoliner.
- Охват сетки задаётся слоем охвата или вычисляется по точкам. Размер ячейки, натяжение, натяжение на границе, коэффициент релаксации, анизотропия, максимум итераций и порог невязки настраиваются в окне команды.
- Расчёт идёт в системе координат слоя: команда запрашивает точки у RustGIS в метрах слоя, а не в градусах.
- Нужен RustGIS 0.7.0 и новее, Windows.

## Установка

1. Скачать `isoliner-0.1.4.rgplugin`.
2. В RustGIS открыть вкладку «Модули» и нажать «Установить из файла».
3. Выбрать пакет и подтвердить установку. Прежнюю версию пакет заменяет.

---

Isoliner for RustGIS 0.1.4 - minimum curvature interpolation.

- A new command, 1.02 Minimum curvature, in the Topography group. Its window takes a point layer and a numeric value field, and adds a float32 GeoTIFF raster on the computed grid to the project. The surface is built from the biharmonic equation with tension, as in the QGIS Isoliner plugin.
- The grid extent is set by an extent layer or computed from the points. Cell size, tension, boundary tension, relaxation factor, anisotropy, maximum iterations and residual threshold are exposed in the command dialog.
- The computation uses the layer's own coordinate system: the command asks RustGIS for features in the layer CRS, not in degrees.
- Needs RustGIS 0.7.0 or later, Windows.

## Install

1. Download `isoliner-0.1.4.rgplugin`.
2. In RustGIS open the Plugins tab and press Install from file.
3. Choose the package and confirm. The package replaces the previous version.
