# Правила для агента (Isoliner for RustGIS)

## Куда складывать собранный плагин

Сборка `.rgplugin` производится скриптом `package.ps1`. Он должен записывать результат только в:

```
C:\Users\Val\Dropbox\ИИ\Isoliner\
```

имя файла по шаблону `isoliner-<version>.rgplugin` (например, `isoliner-0.1.6.rgplugin`).

**Запрещено** сохранять сборки в:
- `C:\Users\Val\Dropbox\Release\` — туда писать нельзя.
- `C:\Users\Val\Dropbox\informpp\` — туда писать нельзя.

## Сборка и публикация

- Перед упаковкой убедиться, что `cargo test` проходит (lib + integration tests).
- `package.ps1` сохранять в UTF-8 BOM, иначе PowerShell 5.1 ломает кириллические пути.
- После изменения `package.ps1` или путей сборки закоммитить изменения и запушить в `origin main`.

## Проверка результата

После сборки проверять:
- файл лежит в `C:\Users\Val\Dropbox\ИИ\Isoliner\`;
- размер соответствует свежей сборке (~1.7 МиБ для v0.1.6);
- `plugin.json` внутри `.rgplugin` содержит актуальный раздел `tab` и группы `tools`.
