Isoliner для RustGIS 0.1.2 - первый выпуск расширения.

Тринадцать инструментов модуля QGIS Isoliner, которым не нужны растр и отметка Z. Каждый принимает те же поля и выдаёт те же значения, что в QGIS, это сверено со всеми прогонами модуля.

- Кригинг - 1.05 «Декластеризация (веса)», 1.06 «Вариограмма (экспериментальная)», 1.08 «Кросс-валидация вариограммы», 1.09 «Кросс-валидация метода (LOO)», демо 1.10 «Пример скважин» и 1.11 «Пример геофизических профилей».
- Топография - 2.08 «Разделить горизонтали для проверки».
- Анализ - демо 3.08 «Пример для плотности».
- Разрез - 4.11 «Образец справочника пластов» и 4.13 «Содержания по пласту в устьях».
- Фракталы - 7.03 «Размерность линий и границ», 7.04 «Размерность Минковского (векторы)» и демо 7.05 «Пример для фракталов».
- У расширения своя вкладка Isoliner на ленте RustGIS, инструменты разложены по меню групп модуля.
- Демо с тем же зерном выдают в RustGIS те же данные, что в QGIS.

Расчёт идёт в метрах зоны UTM, значение Z берётся из поля. HTML-отчётов модуля здесь нет, их цифры выводятся в журнал. Демо 1.10 и 3.08 выдают только векторные слои, растров RustGIS пока не принимает.

## Установка

1. Скачать `isoliner-0.1.2.rgplugin`.
2. В RustGIS открыть вкладку «Модули» и нажать «Установить из файла».
3. Выбрать пакет и подтвердить установку. Прежнюю версию пакет заменяет.

Нужен RustGIS 0.6.0, Windows.

---

Isoliner for RustGIS 0.1.2 - the first release of the extension.

Thirteen tools of the QGIS plugin Isoliner that need neither a raster nor a Z value. Each takes the same fields and gives the same values as in QGIS, checked against every run of the plugin.

- Kriging - 1.05 Declustering (weights), 1.06 Variogram (experimental), 1.08 Variogram cross-validation, 1.09 Method cross-validation (LOO), demos 1.10 Example wells and 1.11 Example geophysical profiles.
- Topography - 2.08 Split contours for validation.
- Analysis - demo 3.08 Example for density.
- Section - 4.11 Bed reference template and 4.13 Bed grades at the collars.
- Fractals - 7.03 Dimension of lines and boundaries, 7.04 Minkowski dimension (vectors) and demo 7.05 Example for fractals.
- The extension has its own Isoliner tab on the RustGIS ribbon, with the tools in menus by plugin group.
- Demos with the same seed give the same data in RustGIS as in QGIS.

Computation runs in UTM zone metres, the Z value is taken from a field. The plugin's HTML reports are not written here, their figures go to the log. Demos 1.10 and 3.08 give vector layers only, as RustGIS does not accept rasters yet.

## Install

1. Download `isoliner-0.1.2.rgplugin`.
2. In RustGIS open the Plugins tab and press Install from file.
3. Choose the package and confirm. The package replaces the previous version.

Needs RustGIS 0.6.0, Windows.
