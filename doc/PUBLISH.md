# Isoliner для RustGIS - журнал публикаций

Общий список площадок и их приёмы - `C:\Dev\planetx-lite\doc\PUBLISH.md`.
Тексты выпуска лежат в `site/release_rustgis_<номер>.md` (описание на
GitHub) и `site/posts_rustgis_<номер>_social.md` (посты).

### Isoliner для RustGIS 0.1.6 - 26 ноября 2026 года

Выпуск по слову автора «двигай по плану». Пять растровых команд:
1.01 «2D Kriging», 3.03 «Вероятность превышения», 3.04 «Гидравлический
градиент», 9.01 «Наклоны и кривизна по оседаниям» и 9.02 «Горизонтальные
деформации». После синхронизации с рабочим компом в main добавлены
дополнительные инструменты (1.04, 1.12, 2.04–2.24, 4.01–4.10), они
войдут в следующие версии. Пакет `isoliner-0.1.6.rgplugin` собран с
тега `rustgis-v0.1.6` (коммит c36749f), SHA-256
`CFC4F9E15ED374603FD8DA9D507C89FC79BA01B6E357FF0ADF66D6F6888CA881`.

| Площадка | Запись |
|---|---|
| GitHub | https://github.com/Valery35/isoliner-rustgis/releases/tag/rustgis-v0.1.6 |
| Dropbox | `ИИ\Isoliner\isoliner-0.1.6.rgplugin` (SHA-256 совпадает с GitHub) |
| Telegram |  |
| Mastodon |  |
| X |  |
| LinkedIn | нужна ручная публикация |
| Facebook |  |
| ВК | нужна ручная публикация |
| Каталог RustGIS |  |
| Сайт informpp.ru | не обновлялся |
| RustGIS автора |  |

### Isoliner для RustGIS 0.1.5 - 25 ноября 2026 года

GitHub release уже создан, но полная раскрутка отложена. Версия
включена в 0.1.6.

| Площадка | Запись |
|---|---|
| GitHub | https://github.com/Valery35/isoliner-rustgis/releases/tag/rustgis-v0.1.5 |
| Dropbox | `ИИ\Isoliner\isoliner-0.1.5.rgplugin` |
| Telegram |  |
| Mastodon |  |
| X |  |
| LinkedIn | нужна ручная публикация |
| Facebook |  |
| ВК | нужна ручная публикация |
| Каталог RustGIS |  |
| Сайт informpp.ru | не обновлялся |
| RustGIS автора |  |

### Isoliner для RustGIS 0.1.4 - 25 ноября 2026 года

Выпуск по слову автора «Публикуй». Новая растровая команда 1.02
«Минимальная кривизна (точки → растр)» в группе «Топография»:
интерполяция точечного слоя методом бигармонии с натяжением, выход -
растр GeoTIFF float32. Расчёт в системе координат слоя через
`host.features`. Нижняя граница RustGIS 0.7.0 сохраняется.

| Площадка | Запись |
|---|---|
| GitHub | https://github.com/Valery35/isoliner-rustgis/releases/tag/rustgis-v0.1.4 |
| Dropbox | `ИИ\Isoliner\isoliner-0.1.4.rgplugin` |
| Telegram | https://t.me/qgis_isolines/238 |
| Mastodon | https://en.osm.town/@Vaery35/117674066044470917 |
| X | https://x.com/Valery35/status/2125373562764607827 |
| LinkedIn | нужна ручная публикация (диалог рекламных объявлений блокирует кнопку) |
| Facebook | https://www.facebook.com/valery35/posts/pfbid038HyKSTN9YBvmFeMue5BJw4fWSrzQPzq8vg1n9TvUf2qbPMCA4QeEH5KAZDQJo5o1l |
| ВК | нужна ручная публикация (канал, не стена) |
| Каталог RustGIS | https://github.com/opengeos/rustgis-plugins/pull/81 |
| Сайт informpp.ru | не обновлялся |
| RustGIS автора | установлена 0.1.4 |

### Isoliner для RustGIS 0.1.2 - 16 ноября 2026 года

Первый выпуск, метка `rustgis-v0.1.2`, по слову автора «Выпускай» в
чате расширения. Тринадцать инструментов модуля, своя вкладка ленты,
нижняя граница RustGIS 0.6.0.

| Площадка | Запись |
|---|---|
| GitHub | https://github.com/Valery35/isoliner-rustgis/releases/tag/rustgis-v0.1.2 - пакет `isoliner-0.1.2.rgplugin`, SHA-256 b45a2508...c652, скачанный с GitHub совпал с местным |
| Dropbox | `ИИ\Isoliner\isoliner-0.1.2.rgplugin`, прежних пакетов расширения там не было |
| Telegram | первый запуск процесса «Telegram о выпуске» прошёл без секретов. Автор завёл их, и 16.11 запуск 43767555057 перезапущен (попытка 2), Telegram ответил ok. Ручной запуск по метке GitHub отклонил («Workflow could not be run»), поэтому перезапускался запуск по выпуску. Пост в канал публикует нажатие автора |
| Mastodon | https://en.osm.town/@Vaery35/117622749668864068 |
| X | https://x.com/Valery35/status/2122090445480292402 |
| LinkedIn | https://www.linkedin.com/feed/update/urn:li:activity:7527856309895966720/ |
| Facebook | https://www.facebook.com/valery35/posts/pfbid0VyQxmGeZNUJz6Ruqy5rEwjXPA7FG9Nm3NC85k7MFfd1E6Px4a7BjLrmtmC5SBrcUl |
| ВК | https://vk.ru/wall-241229428_82 - запись на стене, статьи нет |
| Каталог RustGIS | заявка https://github.com/opengeos/rustgis-plugins/pull/66 (`registry/isoliner.json`, категории Analysis, Vector, Example), по слову автора «Подавай 0.1.2». В пакете 0.1.2 нет `homepage`, поле войдёт в следующую версию |
| Сайт informpp.ru | не обновлялся |
| RustGIS автора | стоит 0.1.1, 0.1.2 не ставилась |
