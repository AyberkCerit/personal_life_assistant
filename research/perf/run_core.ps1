# The core measurements with the model (perf example); in a Terminal tab so they outlive the session.
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_HTTP_MULTIPLEXING = "false"
$env:PLA_LLAMA_SERVER = "C:\dev\PLA\apps\desktop\src-tauri\llama\llama-server.exe"
$env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
$ref = Join-Path $env:TEMP "pla-reference"
$log = Join-Path $env:TEMP "perf_core.log"
# Windows PowerShell's Tee-Object writes UTF-16: each line goes to the screen and, as UTF-8, to the log
Set-Content -Path $log -Value "" -Encoding utf8
cargo run --release -q -p pla-core --example perf -- $ref --model *>&1 | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
Add-Content -Path $log -Value "exit $LASTEXITCODE" -Encoding utf8
