# The core measurements with the model (perf example); in a Terminal tab so they outlive the session.
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_HTTP_MULTIPLEXING = "false"
# the server and the model are not in git: a worktree sets these to the main checkout's copies first
if (-not $env:PLA_LLAMA_SERVER) { $env:PLA_LLAMA_SERVER = Join-Path $root "apps\desktop\src-tauri\llama\llama-server.exe" }
if (-not $env:PLA_MODEL) { $env:PLA_MODEL = Join-Path $root "research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf" }
$ref = Join-Path $env:TEMP "pla-reference"
$log = Join-Path $env:TEMP "perf_core.log"
# Windows PowerShell's Tee-Object writes UTF-16: each line goes to the screen and, as UTF-8, to the log
Set-Content -Path $log -Value "" -Encoding utf8
cargo run --release -q --manifest-path (Join-Path $root "Cargo.toml") -p pla-core --example perf -- $ref --model *>&1 | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
Add-Content -Path $log -Value "exit $LASTEXITCODE" -Encoding utf8
