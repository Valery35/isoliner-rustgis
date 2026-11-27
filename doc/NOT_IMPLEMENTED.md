# Isoliner для RustGIS — нереализованные инструменты модуля QGIS

Состояние на версию 0.1.23. Реализовано 42 инструментов/команд из 79 в модуле QGIS.

## Нереализованные инструменты по группам

### 1. Грид и изолинии

- 1.12 Врезка подробной поверхности в региональную (SurfaceGraftAlgorithm)

### 2. Топография

- 2.01 Загрузка ЦМР по рамке (DemDownloadAlgorithm)
- 2.02 Загрузка топоосновы по рамке (TopobaseDownloadAlgorithm)
- 2.07 Демо-рельеф (TopoDemoReliefAlgorithm)

### 3. Дополнительные инструменты анализа

- 3.01 Категориальный индикаторный кригинг (CategoricalIndicatorAlgorithm)
- 3.02 Кригинг с внешним дрейфом (External Drift) (ExternalDriftKrigingAlgorithm)
- 3.05 Удельный расход (закон Дарси) (DarcyFluxAlgorithm)
- 3.06 Гауссова симуляция (SGS) (SequentialGaussianSimAlgorithm)
- 3.07 Плотность по замерам (переменная опора) (VariableSupportDensityAlgorithm)
- 3.09 Пример для 3.01 и 3.02 (демо) (IndicatorDriftDemoAlgorithm)

### 4. Разрез

- 4.01 Разрез по линии (SectionAlgorithm)
- 4.02 Скважины на разрезе (модель бурения) (DrillholesOnSectionAlgorithm)
- 4.03 Состав пласта на разрезе (CompositionOnSectionAlgorithm)
- 4.04 Пересечение поверхностей с разрезом (SectionGridIntersectAlgorithm)
- 4.05 Пересечение векторов с разрезом (SectionVectorIntersectAlgorithm)
- 4.06 Пересечение TIN с разрезом (SectionTinIntersectAlgorithm)
- 4.07 Проекция объектов на разрез (SectionProjectAlgorithm)
- 4.08 Спроецировать с разреза (SectionUnprojectAlgorithm)
- 4.09 Развёртка стенки ствола (бета) (ShaftUnwrapAlgorithm)
- 4.10 Пример для разреза (демо) (SectionDemoAlgorithm)
- 4.12 Элементы залегания по следу выхода (AttitudeFromTraceAlgorithm)

### 5. Геологическая модель (бета)

- 5.01 Согласованность пачки пластов (StackCheckAlgorithm)
- 5.02 Пример разреза (демо) (StackDemoAlgorithm)
- 5.03 Сборка пачки от рельефа (StackBuildAlgorithm)
- 5.04 Коррекция пачки по статистике мощностей (StackFixAlgorithm)
- 5.05 Манифест модели (ManifestAlgorithm)
- 5.06 Складчатость поверхности (FoldMapAlgorithm)

### 6. Гидрология

- 6.08 Створы и кривые расходов (RatingCurveAlgorithm)
- 6.09 Полигон затопления (FloodExtentAlgorithm)
- 6.10 Импорт таблиц створов (ImportSectionTableAlgorithm)
- 6.11 Пример реки (демо) (DemoRiverAlgorithm)

### 7. Фрактальный анализ

- 7.01 Фрактальная размерность (FractalDimensionAlgorithm)
- 7.02 Box-counting маски (BoxCountingAlgorithm)

### 8. Обмен данными

- 8.01 Принять LandXML (LandXmlReadAlgorithm)
- 8.02 Записать LandXML (LandXmlWriteAlgorithm)
- 8.03 Пример LandXML (демо) (LandXmlDemoAlgorithm)

### 9. Сдвижения

- 9.03 Пример мульды (демо) (SubsidenceDemoAlgorithm)

## Примечания

- Растровые инструменты реализуются как команды RustGIS, потому что в версии 0.8.0 инструмент расширения не принимает и не возвращает растры.
- Демо-инструменты с растровым выходом в QGIS здесь пока выдают только векторные слои.
- Номера и названия групп приведены в соответствие с модулем QGIS `grid_isolines/algorithms.py`.
