"""Rust çekirdeği (crates/pla-core) için çıkarım varlıklarını ve eşlik fikstürlerini üretir.

Çalıştırma (bu klasörde): .venv/Scripts/python export_rust_assets.py
Gerekli: data/eval.jsonl, data/test.jsonl (dataset.py / testset.py üretir) ve
results/gemma4-e2b-q3km.v3.lowmem{,.test}.jsonl (F1 koşuları).
"""
import json
import os
from datetime import date
from pathlib import Path

os.environ["PROMPT"] = "v3"
os.environ["WHICH_DEFAULT"] = "1"
import run_eval as R  # noqa: E402  (ortam değişkenleri içe aktarmadan önce ayarlanmalı)

HERE = Path(__file__).parent
CORE = HERE.parent.parent / "crates" / "pla-core"
RUNS = (("eval", "gemma4-e2b-q3km.v3.lowmem.jsonl"), ("test", "gemma4-e2b-q3km.v3.lowmem.test.jsonl"))


def dump(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main():
    dump(CORE / "assets" / "extraction_schema_v3.json", R.SCHEMA)
    dump(CORE / "assets" / "extraction_prompt_v3.json", {
        "system": R.SYSTEM_V2,
        "few_shot": [{"user": R.user_msg(r, t), "assistant": json.dumps(o, ensure_ascii=False)}
                     for r, t, o in R.FEW_SHOT_V2],
    })
    sample_ref, sample_text = "2026-10-06", "yarın 9da dişçi var"
    dump(CORE / "tests" / "fixtures" / "messages_sample.json",
         {"ref": sample_ref, "text": sample_text, "messages": R.messages(sample_ref, sample_text)})

    cases = []
    for data, results in RUNS:
        notes = [json.loads(l) for l in (HERE / "data" / f"{data}.jsonl").read_text(encoding="utf-8").splitlines()]
        refs = {n["id"]: n["ref"] for n in notes}
        for line in (HERE / "results" / results).read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            try:
                items = json.loads(row["raw"])["items"]
            except (json.JSONDecodeError, KeyError, TypeError):
                continue
            ref = date.fromisoformat(refs[row["id"]])
            for it in items:
                d = R.resolve_date(it, ref)
                cases.append({
                    "id": f"{data}/{row['id']}",
                    "ref": ref.isoformat(),
                    "item": it,
                    "date": d if d is None or d == "invalid" else d.isoformat(),
                    "valid": R.validate(it),
                    "value": R.canonical_value(it.get("metric") or {}) if it.get("type") == "metric" else None,
                })
    dump(CORE / "tests" / "fixtures" / "parity_v3.json", cases)
    print(f"{len(cases)} parity cases")


if __name__ == "__main__":
    main()
