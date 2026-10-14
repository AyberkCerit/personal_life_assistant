# The window measurements on the release build (perf_ui.py); a Terminal tab outlives the session.
$env:PLA_EXE = "C:\dev\PLA\target\release\pla-desktop.exe"
$env:PYTHONIOENCODING = "utf-8"
$ref = Join-Path $env:TEMP "pla-reference"
$log = Join-Path $env:TEMP "perf_ui.log"
Set-Content -Path $log -Value "" -Encoding utf8
& "C:\Users\ayber\miniconda3\python.exe" -u C:\dev\PLA\research\perf\perf_ui.py $ref *>&1 | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
Add-Content -Path $log -Value "exit $LASTEXITCODE" -Encoding utf8
