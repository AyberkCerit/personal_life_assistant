# Contributing to PLA

PLA has one developer, so small and specific contributions help most. Issues can be written in English or Turkish.

## Report a note PLA got wrong

This is the most useful contribution. Open a [wrong result](https://github.com/AyberkCerit/personal_life_assistant/issues/new?template=wrong-result.yml) issue with the note or question, what you expected, and what PLA did.

Issues are public. Your notes may hold health data, names and places, so rewrite them first and keep only the wording that confused the model.

## Add a test case

If you can write the case yourself, add it to `crates/pla-core/tests/nl/`:

| File | Covers |
|---|---|
| `extraction.jsonl` | One block of a note, read by the model and validated |
| `assistant.jsonl` | Questions and tool calls in the assistant panel |
| `memory.jsonl` | What the assistant remembers from a chat |

The comment at the top of each file explains its keys. An extraction case looks like this:

```json
{"id":"prog-guc1","cat":"program","text":"Güç 1 (Pazar)\n- Bench press: 4x4-6","expect":{"none_of":["workout"]}}
```

Add new cases instead of editing existing ones, because the results in `research/` depend on them. A case that fails today is welcome, so say so in the pull request.

These cases need the real model and aren't part of `cargo test`. To run them, set `PLA_LLAMA_SERVER` and `PLA_MODEL`, then:

```bash
cargo run --release -p pla-core --example nl_eval -- my-run extraction
```

Replace `extraction` with `assistant`, `memory` or the start of a case id to run a different set.

## Share a benchmark

Runs on other hardware help, especially slower laptops and AMD CPUs. The commands are in [`BENCHMARKS.md`](BENCHMARKS.md#reproduce). Post your numbers in a [benchmark result](https://github.com/AyberkCerit/personal_life_assistant/issues/new?template=benchmark.yml) issue.

## Change the code

The [README](README.md#install) covers setup. Before opening a pull request, run:

```bash
cargo test --workspace
cd apps/desktop && npm run check && npm test
```

Some Rust files don't match `rustfmt.toml` yet, so don't run `cargo fmt` across the repository. Keep formatting changes to the code you touched.

For anything larger than a small fix, open an issue first so we can agree on the approach.

Some decisions are part of what PLA promises, so a change to any of them needs an issue first:

- **Network:** PLA only goes online to download a model. Don't add telemetry, update checks or new outbound connections.
- **Deleting:** automation and the assistant's tools never delete notes or records.
- **Arithmetic:** Rust computes dates, sums and averages. The model only writes text.
- **AI marking:** whatever the model adds keeps its ✦ badge, and nothing else uses that mark.

Read [`DESIGN.md`](DESIGN.md) before changing a screen. Colours come from the theme tokens, and a test fails on raw colour values.

To add an interface language, start with `apps/desktop/src/lib/i18n.ts`. It currently holds Turkish and English.

Commit messages are in English and follow [Conventional Commits](https://www.conventionalcommits.org/), for example `fix(index): parse existing notes again`.

## License

By contributing, you agree that your work is released under the [MIT License](LICENSE).
