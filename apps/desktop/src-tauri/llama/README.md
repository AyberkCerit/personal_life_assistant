# Bundled llama-server

This folder ships with PLA as a Tauri resource (`tauri.conf.json > bundle.resources`).
Its binaries are not in git; fill it with:

    cd apps/desktop
    npm run fetch-llama -- --from <folder with llama.cpp b11280, Windows x64 CPU binaries>

Without them PLA still builds and runs; it then needs `PLA_LLAMA_SERVER` (or the
`llama_server` setting) to use a language model.
