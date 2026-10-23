# The window measurements on the release build (perf_ui.py); a Terminal tab outlives the session.
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$env:PLA_EXE = Join-Path $root "target\release\pla-desktop.exe"
$env:PYTHONIOENCODING = "utf-8"
$ref = Join-Path $env:TEMP "pla-reference"
$log = Join-Path $env:TEMP "perf_ui.log"
Set-Content -Path $log -Value "" -Encoding utf8
& "C:\Users\ayber\miniconda3\python.exe" -u (Join-Path $PSScriptRoot "perf_ui.py") $ref *>&1 | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
Add-Content -Path $log -Value "exit $LASTEXITCODE" -Encoding utf8
