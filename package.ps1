# Isoliner for RustGIS - plugin packaging.
# Copyright (C) 2026 Inform++ LLC. License Apache 2.0.
# SPDX-License-Identifier: Apache-2.0

param(
    [string]$OutDir = "$env:USERPROFILE\Dropbox\ИИ\Isoliner"
)

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
$repo = $PSScriptRoot
$target = "$repo\target\release\isoliner-rustgis.exe"

Write-Host "Building release..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

$version = (Get-Content "$repo\Cargo.toml" | Select-String '^version\s*=\s*"(.+)"').Matches.Groups[1].Value
$pluginDir = "$repo\target\plugin\isoliner"
Remove-Item -Recurse -Force $pluginDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$pluginDir\bin" | Out-Null
Copy-Item $target "$pluginDir\bin\isoliner-rustgis.exe"

# Generate plugin.json with the tools section (UTF-8, not PowerShell's default UTF-16 LE).
& "$target" manifest | Out-File -FilePath "$pluginDir\plugin.json" -Encoding UTF8
if ($LASTEXITCODE -ne 0) { throw "manifest generation failed" }

$zipName = "isoliner-${version}"
$zipPath = "$OutDir\${zipName}.rgplugin"
$zipTemp = "$OutDir\${zipName}.zip"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Remove-Item -Force $zipPath -ErrorAction SilentlyContinue
Remove-Item -Force $zipTemp -ErrorAction SilentlyContinue
Compress-Archive -Path "$pluginDir\*" -DestinationPath $zipTemp
Move-Item -Force $zipTemp $zipPath

Write-Host "Done: $zipPath"
