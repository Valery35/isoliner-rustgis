Isoliner для RustGIS 0.1.6 - четыре новые растровые команды.

- Команда 1.01 «2D Kriging» (точки → растр): ординарный или простой кригинг с наггетом и одной структурой вариограммы, снятие тренда, логарифмирование, растр стандартной ошибки, отсев ураганных проб.
- Команда 3.03 «Вероятность превышения»: P(Z > t) или P(Z < t) по растру оценки и растру стандартной ошибки кригинга.
- Команда 3.04 «Гидравлический градиент»: модуль градиента напора и азимут направления потока по растру напора.
- Команда 9.01 «Наклоны и кривизна по оседаниям»: наклон, азимут наклона, кривизна вдоль наклона и главные кривизны по гриду оседаний.
- Команда 9.02 «Горизонтальные деформации»: оценка ε = mₑ·K·L по растру кривизны из 9.01.
- Все команды работают в системе координат слоя и добавляют результаты растрами GeoTIFF float32 с nodata -9999.
- Нужен RustGIS 0.7.0 и новее, Windows.

## Установка

1. Скачать `isoliner-0.1.6.rgplugin`.
2. В RustGIS открыть вкладку «Модули» и нажать «Установить из файла».
3. Выбрать пакет и подтвердить установку. Прежнюю версию пакет заменяет.

---

Isoliner for RustGIS 0.1.6 - four new raster commands.

- Command 1.01 2D Kriging (points to raster): ordinary or simple kriging with a nugget and one variogram structure, detrend, log transform, standard-error raster, outlier clipping.
- Command 3.03 Probability of exceedance: P(Z > t) or P(Z < t) from an estimate raster and a kriging standard-error raster.
- Command 3.04 Hydraulic gradient: head-gradient magnitude and flow-direction azimuth from a head raster.
- Command 9.01 Subsidence tilt and curvature: tilt, tilt azimuth, curvature along tilt and principal curvatures from a subsidence grid.
- Command 9.02 Horizontal deformation: estimate ε = mₑ·K·L from the curvature raster of 9.01.
- All commands run in the layer CRS and add results as float32 GeoTIFF rasters with nodata -9999.
- Needs RustGIS 0.7.0 or later, Windows.

## Install

1. Download `isoliner-0.1.6.rgplugin`.
2. In RustGIS open the Plugins tab and press Install from file.
3. Choose the package and confirm. The package replaces the previous version.
