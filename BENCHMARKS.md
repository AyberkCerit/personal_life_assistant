# PLA benchmarks

Every number here comes from a results file in [`research/`](research) (written in Turkish) and can be rerun with the commands at the end. Several sets are small, so the case count is next to each result. Read the small ones as indicators, not as precise rates.

Hardware for the model runs: an Intel i7-13620H with the model limited to 3 or 4 CPU threads and no GPU. Model: Gemma 4 E2B-it, Q3_K_M, on llama.cpp `llama-server` b11280.

## 1. Choosing the extraction model

The task: read a note such as "yarin 8de spor" or "gym tmrw 8am, ran 5k" and return structured items with a relative date, through a JSON-schema grammar. Five models were scored on a development set of 100 Turkish and 100 English notes. To be picked, a model had to reach 80% field accuracy in both languages within a 2 GB memory budget. Field accuracy is the share of expected items extracted exactly right.

<img src="docs/readme/model-shootout.svg" alt="Bar chart. Field accuracy Turkish/English: Qwen2.5 1.5B 46/61%, Qwen3 1.7B 64/76%, Qwen3 4B 84/86%, Gemma 4 E2B Q4_0 QAT 97/95%, Gemma 4 E2B Q3_K_M 96/95%. Working memory: 1.91, 2.42, 5.04, 2.75 and 1.85 GB." width="100%">

| Model | Quant | TR | EN | Working memory | p95 per note |
|---|---|---:|---:|---:|---:|
| Qwen2.5 1.5B | Q4_K_M | 46% | 61% | 1.91 GB | 2.7 s |
| Qwen3 1.7B | Q4_K_M | 64% | 76% | 2.42 GB | 3.6 s |
| Qwen3 4B Instruct 2507 | Q4_K_M | 84% | 86% | 5.04 GB | 8.4 s |
| Gemma 4 E2B-it | Q4_0 QAT | 97% | 95% | 2.75 GB | 4.1 s |
| Gemma 4 E2B-it (shipped) | Q3_K_M | 96% | 95% | 1.85 GB | 4.9 s |

Working memory is what the `llama-server` process holds while extracting (2048-token context, flash attention, 8-bit KV cache). The Q3_K_M file itself is a 2.5 GB download.

After the prompt was frozen, I wrote a second set of 100 Turkish and 100 English notes that shares no note with the development set, with different reference days and sloppy spelling ("yarin 8de", "gym tmrw"). Nothing in the prompt changed after that.

| Held-out set (200 notes) | TR | EN |
|---|---:|---:|
| Field accuracy | 91% | 91% |
| Precision | 93% | 96% |
| Date / time correct | 94% / 98% | 97% / 100% |
| Metric value correct | 97% | 100% |
| Notes with nothing to extract, left empty | 100% | 100% |
| p50 / p95 per note | 2.1 / 4.7 s | 2.0 / 4.4 s |

The MVP acceptance bar is 85% on this held-out set in each language.

What the runs showed:

- With grammar-constrained output, all five models produced valid JSON every time. The errors were about meaning.
- llama.cpp's grammar generates properties in schema order and cannot go back to an optional field it skipped. With `value` listed before `exercise`, `sets` and `reps`, workout weights went missing; moving it fixed that.
- The model returns relative time (`{"day_offset": 1}`, `{"weekday": "fri", "which": "next"}`) and Rust resolves it against the note's own date, so "tomorrow" in yesterday's note lands on the right day.
- The 17 held-out notes that failed mostly involve past weekdays in metrics ("I was 80 kg on Friday"), English event sentences ("registration opens on…") not read as tasks, and items skipped in notes with several of them.

Source: [`research/f1-model-eval/RESULTS.md`](research/f1-model-eval/RESULTS.md)

## 2. Performance on a 10,000-note vault

`make_reference` generates a deterministic vault: five years of daily notes, one 1 MB note, about 900 MB of attachments, 20,000 tasks and 50,000 measurements. Limits come from the requirements spec (NFR-PERF-001 to 012) and were set before measuring. Release build, model on 3 threads, which is what PLA gives it on a 4-core machine.

<img src="docs/readme/perf-budgets.svg" alt="Measured p95 as a share of each limit. Cold start 1.72 s of 3 s; open a 1 MB note 72 ms of 200 ms; keystroke to frame while the model is busy 7.7 ms of 16 ms; full-text search 33 ms of 300 ms; queries 12 ms of 100 ms; model load 2.6 s of 5 s; window from tray 641 ms of 3 s; first index 165 s of 300 s; memory in tray 27 MB of 150 MB; memory with the window open and idle 374 MB of 450 MB." width="100%">

| Measurement (p95) | Result | Limit |
|---|---:|---:|
| Cold start to an editable note | 1.72 s | 3 s |
| Open a 1 MB note | 72 ms | 200 ms |
| Keystroke to frame while the model answers | 7.7 ms (worst 46 ms) | 16 ms |
| Full-text search | 33 ms | 300 ms |
| Task, metric and report queries | 12 ms or less | 100 ms |
| Memory in the tray, no model | 27 MB | 150 MB |
| Memory with the window open and idle | 374 MB | 450 MB |
| Model load | 2.6 s | 5 s |
| Extract a note with 10 changed blocks | 46.5 s | 60 s |
| First answer, no tool call (4 runs) | 42 s | 45 s |
| First answer with a tool call (6 runs) | 64 s | 70 s |
| First index of the vault | 165 s | 300 s |
| Open the window from the tray | 641 ms | 3 s |

Three of these were over the limit before the last round of work:

| | Before | After | Change |
|---|---:|---:|---|
| Memory in the tray | ~1 GB | 27 MB | The WebView is destroyed when the window closes to the tray and rebuilt when it reopens. |
| Cold start | 4.2 s | 1.72 s | The file tree drew 20,056 rows (90,000 DOM nodes, 60,000 listeners) at startup. Folders now start collapsed. |
| Open a 1 MB note | 406 ms | 72 ms | Same cause as the cold start. |

The assistant is slow on a CPU. Prompt processing runs at about 50 tokens/s on 3 threads and generation at about 25 tokens/s, and every question adds about 1,700 tokens (retrieved notes plus instructions). PLA reads the ~2,400-token prefix that every question shares (rules, calendar, the last 48 hours of notes) as soon as you click into the question box, so a question only pays for its own part. Before that change the first answer took 79 to 88 s. The initial target was 2 s; the spec now says 45 s, and 70 s with a tool call. Trimming retrieved context would save about 10 s and cost answer quality, so I kept it.

Source: [`research/perf/RESULTS.md`](research/perf/RESULTS.md)

## 3. The assistant on real phrasings

`nl_eval` runs the app's own assistant code (`qa::engine::answer`) against the real model. The first cases were messages that went wrong in daily use: "selam", "130 kiloyum", "notlarıma ekle bunu", "sen karar ver", a pasted gym program. Variations followed in both languages. The prompt's example bank may not contain any case sentence; the harness checks this.

Run of 10 October 2026, 122 cases:

| Category | First run | Latest |
|---|---:|---:|
| Answer in the user's language | 8/14 | 14/14 |
| Log a measurement | 5/13 | 13/13 |
| Add a task | 7/9 | 8/9 |
| Complete a task from "I did X" | 1/3 | 3/3 |
| Take a note | 7/10 | 10/10 |
| Suggest a note | 3/10 | 9/10 |
| Questions over your data | 8/9 | 9/9 |
| Follow-up questions | 2/3 | 3/3 |
| Extraction, block by block | 26/31 | 31/31 |
| Must not write anything | | 5/5 |
| Held-out phrasings | | 12/15 |
| Total | 67/102 | 117/122 |

Many of the older cases are close to sentences in the example bank, so 117/122 mostly measures known patterns. The held-out cases were written without looking at the bank, and the harness rejects any that shares more than half its word stems with an example. On those, expect 80 to 87%.

Open failures include a message that mixes a measurement and a task ("dün 7 saat uyudum, yarın da 9'da toplantı var"), where only one of the two is recorded.

The suite has grown since: [`crates/pla-core/tests/nl/`](crates/pla-core/tests/nl) now holds 133 assistant, 48 extraction and 122 memory-extraction cases.

Sources: [`research/nl-eval/RESULTS.md`](research/nl-eval/RESULTS.md), and for the persona settings (name, form of address, sen/siz), [`research/persona-eval/RESULTS.md`](research/persona-eval/RESULTS.md) with 24 of 25 cases passing.

## 4. Retrieval through links

When you ask who advises your capstone project, the answer is often in a note that the project note links to. The test vault has 36 hand-written notes and 300 generated daily notes that mention the same topics and link to the same hub notes but contain none of the facts. 22 cases: in 12 the answer is in a linked note, in 10 it is in the note the question names. This measures whether the answer reaches the model's context, not the answer itself.

| Mode | Linked (12) | Direct (10) | Avg. tokens retrieved | Time |
|---|---:|---:|---:|---:|
| Keywords (FTS5) | 7 | 10 | 1,143 | 0.8 ms |
| Keywords + outgoing links | 10 | 10 | 1,328 | 1.3 ms |
| Hybrid (FTS5 + EmbeddingGemma 300M) | 10 | 10 | 1,316 | 2.5 ms |
| Hybrid + outgoing links (shipped) | 11 | 10 | 1,472 | 2.9 ms |

Following incoming links as well gained nothing: hub notes have hundreds of backlinks from daily notes, and those pushed out the note with the answer. One caveat: the cases, the generated notes and the algorithm come from the same author, and the link structure is tidier than a real vault's. It still needs a run on a real vault with real questions.

Source: [`research/retrieval-eval/RESULTS.md`](research/retrieval-eval/RESULTS.md)

## 5. Assistant memory

The assistant keeps a few short Markdown files under `memory/` (people, preferences, current state) that you can read and edit like any note. The gate run on 23 October 2026 used all 152 assistant and extraction cases that existed then:

| | Without memory | With memory |
|---|---:|---:|
| Questions only memory can answer (13) | 0/13 | 9/13 |
| Older cases, checking for regressions (138) | 122/138 | 121/138 |

The one older case that changed answered an English question in Turkish. Retrieval with memory loaded stayed at 21/22, because memory has its own share of the context budget.

Source: [`research/memory-eval/RESULTS.md`](research/memory-eval/RESULTS.md)

## 6. MVP acceptance

| Criterion | Status |
|---|---|
| Extraction: valid JSON 100%, field accuracy at least 85% in TR and EN | Met, 91% / 91% held-out |
| A typo fix creates no duplicate; edited or rejected items are not recreated | Met |
| Memory within budget when idle and while the model runs | Met |
| Automation deletes no raw data; a copy of the vault alone restores everything | Met |
| A weekly report missed while the computer was off is caught up; missed reminders are listed on start | Met |
| Answers cite notes, don't invent, and numbers match SQL exactly | Met |
| I use PLA as my only notes and task app for four weeks | Running until 13 November 2026 |

Of the 198 high-priority requirements in the spec, 147 are referenced by ID in code or tests; the rest were checked one by one. Source: [`research/acceptance/RESULTS.md`](research/acceptance/RESULTS.md)

## Reproduce

```bash
# Performance: build the stress vault, then measure (add --model with PLA_LLAMA_SERVER and PLA_MODEL set)
cargo run --release -p pla-core --example make_reference -- ./ref
cargo run --release -p pla-core --example perf -- ./ref --model

# Assistant and extraction suite against the real model
cargo run --release -p pla-core --example nl_eval -- my-run

# Retrieval (keywords only without an embedding model, hybrid with PLA_EMBED_MODEL set)
cargo run --release -p pla-core --example retrieval_eval -- my-run

# Model comparison (Python + llama-server)
python research/f1-model-eval/run_eval.py gemma4-e2b-q3km
```

The charts in this file are generated from these numbers by `python docs/readme/make_svgs.py`.
