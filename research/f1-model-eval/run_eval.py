"""F1 model fizibilite denemesi: llama-server + JSON şemasıyla kısıtlı çıkarım.

Kullanım: .venv/Scripts/python.exe run_eval.py [model_anahtari ...]
Her model için: 4 iş parçacığı, GPU kapalı, 4096 bağlam; data/eval.jsonl üzerinde ölçüm.
Çıktı: results/<anahtar>.jsonl (örnek bazında) ve results/summary.json
"""
import json
import statistics
import subprocess
import sys
import threading
import os
import time
from datetime import date, timedelta
from pathlib import Path

import psutil
import requests

HERE = Path(__file__).parent
BIN = HERE / "bin" / "llama-server.exe"
MODELS = {
    "qwen2.5-1.5b": "qwen2.5-1.5b-instruct-q4_k_m.gguf",
    "qwen3-1.7b": "Qwen3-1.7B-Q4_K_M.gguf",
    "gemma4-e2b": "gemma-4-E2B_q4_0-it.gguf",
    "qwen3-4b-2507": "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
    "gemma4-e2b-q3km": "gemma-4-E2B-it-Q3_K_M.gguf",
    "gemma4-e2b-iq4xs": "gemma-4-E2B-it-IQ4_XS.gguf",
}
THREADS, PORT = 4, 8765
CTX = int(os.environ.get("CTX", "4096"))
EXTRA = os.environ.get("EXTRA", "").split()
TAG = os.environ.get("TAG", "")
DATA = os.environ.get("DATA", "eval")
WHICH_DEFAULT = os.environ.get("WHICH_DEFAULT") == "1"  # SRS FR-EXT-028
PROMPT = os.environ.get("PROMPT", "v1")
WEEKDAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
DAY_NAMES = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]

SCHEMA = {
    "type": "object", "additionalProperties": False, "required": ["items"],
    "properties": {"items": {"type": "array", "maxItems": 5, "items": {
        "type": "object", "additionalProperties": False, "required": ["type"],
        "properties": {
            "type": {"enum": ["task", "reminder", "metric"]},
            "title": {"type": "string", "maxLength": 120},
            "when": {"type": "object", "additionalProperties": False, "properties": {
                "day_offset": {"type": "integer", "minimum": -30, "maximum": 365},
                "weekday": {"enum": WEEKDAYS},
                "which": {"enum": ["this", "next"]},
                "date": {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$"},
                "time": {"type": "string", "pattern": "^[0-9]{2}:[0-9]{2}$"}}},
            "metric": {"type": "object", "additionalProperties": False, "properties": {
                "kind": {"enum": ["sleep", "water", "steps", "weight", "workout"]},
                "value": {"type": "number"},
                "unit": {"enum": ["h", "min", "ml", "l", "glass", "count", "kg", "lb"]},
                "exercise": {"type": "string", "maxLength": 60},
                "sets": {"type": "integer", "minimum": 1, "maximum": 50},
                "reps": {"type": "integer", "minimum": 1, "maximum": 500}}}}}}},
}

# v3: gramer özellikleri tanım sırasıyla ürettiği için "value" egzersiz bilgisinden sonra gelir
if PROMPT == "v3":
    _m = SCHEMA["properties"]["items"]["items"]["properties"]["metric"]["properties"]
    SCHEMA["properties"]["items"]["items"]["properties"]["metric"]["properties"] = {k: _m[k] for k in ("kind", "exercise", "sets", "reps", "value", "unit")}

SYSTEM = """You extract structured items from one personal note. Answer with JSON only.

Create an item ONLY for:
- a concrete future action, appointment or deadline -> type "task" (use "reminder" if the note asks to be reminded or says "unutma"/"don't forget"),
- a measured personal health value -> type "metric".
Diary entries, feelings, opinions, ideas and past events without a measurement produce NO items: {"items": []}.

title: short, in the language of the note.
when:
- relative days -> day_offset (today 0, tomorrow/yarın 1, day after tomorrow/öbür gün 2, in N days N, yesterday/dün -1, evvelsi gün -2).
- weekday names -> weekday + which. which="this" for the upcoming occurrence; which="next" only when the note says "haftaya", "next <weekday>" or "<weekday> next week".
- explicit calendar dates -> date as YYYY-MM-DD using the reference year.
- a past weekday (e.g. "I was 79 kg on Monday") -> negative day_offset.
- time: 24-hour HH:MM, only if a clock time is stated ("akşam 7" = 19:00, "öğleden sonra 3" = 15:00, "öğlen"/"noon" = 12:00, "buçuk"/"half" = :30). Never invent a time.
metric: kind is sleep (unit h), water (unit ml, l or glass), steps (unit count), weight (unit kg or lb) or workout (exercise, sets, reps, value = weight in kg if stated, unit kg).
A metric without a stated day is for today: omit "when".
Produce one item per distinct action or measurement."""

FEW_SHOT = [
    ("2026-06-10", "Yarın 10'da berbere git, dün de 6 saat uyudum.",
     {"items": [{"type": "task", "title": "Berbere git", "when": {"day_offset": 1, "time": "10:00"}},
                {"type": "metric", "when": {"day_offset": -1}, "metric": {"kind": "sleep", "value": 6, "unit": "h"}}]}),
    ("2026-06-10", "Call the plumber on Thursday at 5 pm. Drank 4 glasses of water.",
     {"items": [{"type": "task", "title": "Call the plumber", "when": {"weekday": "thu", "which": "this", "time": "17:00"}},
                {"type": "metric", "metric": {"kind": "water", "value": 4, "unit": "glass"}}]}),
    ("2026-06-10", "Bugün çok güzel bir gündü, akşam film izledik.", {"items": []}),
    ("2026-06-10", "Leg press 3x12 at 90 kg. Haftaya pazartesi fizyoterapi randevusu.",
     {"items": [{"type": "metric", "metric": {"kind": "workout", "exercise": "leg press", "sets": 3, "reps": 12, "value": 90, "unit": "kg"}},
                {"type": "task", "title": "Fizyoterapi randevusu", "when": {"weekday": "mon", "which": "next"}}]}),
]


SYSTEM_V2 = SYSTEM + """

Weekday names (use exactly these codes): pazartesi/Monday=mon, salı/Tuesday=tue, çarşamba/Wednesday=wed, perşembe/Thursday=thu, cuma/Friday=fri, cumartesi/Saturday=sat, pazar/Sunday=sun.
Tasks are also stated with "lazım", "gerek", "var", "-meli/-malı", or just a noun with a day (e.g. "Cuma günü proje teslimi") -> always create a task for them.
If only a clock time is given for a task ("akşam 8'de ...", "bu akşam 21:30"), set day_offset 0.
A workout is ONE metric item: put exercise, sets, reps and the weight (value, unit kg) together; never create a separate weight item for it.
Past days apply to metrics too: "dün 12000 adım" -> when.day_offset -1.
Read numbers carefully: "10 bin" = 10000, "8.500" = 8500, "1,5 litre" = 1.5 l, "yarım litre" = 0.5 l.
For sleep between two clock times compute the hours (23:00 -> 06:30 = 7.5)."""

FEW_SHOT_V2 = FEW_SHOT + [
    ("2026-06-10", "Salı günü kargo teslim alınacak, perşembe 20:00'de maç var. Dün 9 bin adım attım.",
     {"items": [{"type": "task", "title": "Kargoyu teslim al", "when": {"weekday": "tue", "which": "this"}},
                {"type": "task", "title": "Maç", "when": {"weekday": "thu", "which": "this", "time": "20:00"}},
                {"type": "metric", "when": {"day_offset": -1}, "metric": {"kind": "steps", "value": 9000, "unit": "count"}}]}),
    ("2026-06-10", "Akşam 9'da çöpleri çıkar. Deadlift 4x6 110 kg yaptım.",
     {"items": [{"type": "task", "title": "Çöpleri çıkar", "when": {"day_offset": 0, "time": "21:00"}},
                {"type": "metric", "metric": {"kind": "workout", "exercise": "deadlift", "sets": 4, "reps": 6, "value": 110, "unit": "kg"}}]}),
]


def user_msg(ref, text):
    d = date.fromisoformat(ref)
    return f"Reference date: {ref} ({DAY_NAMES[d.weekday()]})\nNote: {text}"


def messages(ref, text):
    m = [{"role": "system", "content": SYSTEM_V2 if PROMPT in ("v2", "v3") else SYSTEM}]
    for r, t, out in (FEW_SHOT_V2 if PROMPT in ("v2", "v3") else FEW_SHOT):
        m.append({"role": "user", "content": user_msg(r, t)})
        m.append({"role": "assistant", "content": json.dumps(out, ensure_ascii=False)})
    m.append({"role": "user", "content": user_msg(ref, text)})
    return m


# ------------------------------------------------------------------ Rust'ta uygulanacak kuralların Python eşleniği
def resolve_date(item, ref):
    w = item.get("when") or {}
    try:
        if "date" in w:
            return date.fromisoformat(w["date"])
        if "day_offset" in w:
            return ref + timedelta(days=w["day_offset"])
        if "weekday" in w:
            ahead = (WEEKDAYS.index(w["weekday"]) - ref.weekday()) % 7
            return ref + timedelta(days=ahead + (7 if w.get("which") == "next" else 0))
    except ValueError:
        return "invalid"
    if item.get("type") == "metric" or (PROMPT in ("v2", "v3") and "time" in w):
        return ref
    return None


def canonical_value(m):
    v, u = m.get("value"), m.get("unit")
    if v is None:
        return None
    k = m.get("kind")
    if k == "water":
        return v * 250 if u == "glass" else v * 1000 if u == "l" or (u is None and v < 10) else v
    if k == "workout" and (v <= 0 or u not in (None, "kg", "lb")):
        return None  # yalnız pozitif kg/lb ağırlık sayılır ("value 0, unit count" şınav, "1 h" spor)
    if k in ("weight", "workout"):  # SRS E-D7: lb -> kg
        return v * 0.45359237 if u == "lb" else v
    if k == "sleep":
        return v / 60 if u == "min" else v
    return v


def validate(item):
    """Rust'ın anlamsal doğrulaması: geçemeyen öğe İnceleme kutusuna düşer."""
    t = item.get("type")
    w = item.get("when") or {}
    if "time" in w:
        hh, mm = map(int, w["time"].split(":"))
        if hh > 23 or mm > 59:
            return False
    if sum(k in w for k in ("day_offset", "date", "weekday")) > 1:
        return False
    if "weekday" in w and "which" not in w and not WHICH_DEFAULT:
        return False
    if "date" in w:
        try:
            date.fromisoformat(w["date"])
        except ValueError:
            return False
    if t in ("task", "reminder"):
        return bool((item.get("title") or "").strip()) and "metric" not in item
    m = item.get("metric") or {}
    if m.get("kind") is None:
        return False
    if m["kind"] == "workout":
        return m.get("sets") is not None or m.get("reps") is not None or canonical_value(m) is not None
    v = canonical_value(m)
    ranges = {"sleep": (0, 24), "water": (0, 10000), "steps": (0, 100000), "weight": (20, 400)}  # SRS E-D8
    return v is not None and ranges[m["kind"]][0] <= v <= ranges[m["kind"]][1]


def date_ok(gold_d, pred, ref):
    if isinstance(gold_d, list):
        return any(date_ok(g, pred, ref) for g in gold_d)
    if gold_d is None:
        return pred is None
    if isinstance(gold_d, str):
        return pred == date.fromisoformat(gold_d)
    return pred == ref + timedelta(days=gold_d)


def time_ok(gold_t, pred_t):
    if isinstance(gold_t, list):
        return pred_t in gold_t
    return pred_t == gold_t


def item_correct(g, p, ref):
    pd = resolve_date(p, ref)
    if g["t"] == "action":
        return {"type": True, "date": date_ok(g["d"], pd, ref), "time": time_ok(g["time"], (p.get("when") or {}).get("time"))}
    m = p.get("metric") or {}
    val = canonical_value(m)
    v_ok = g["value"] is None or (val is not None and abs(val - g["value"]) <= max(0.1, 0.02 * g["value"]))
    sr_ok = (g["sets"] is None or m.get("sets") == g["sets"]) and (g["reps"] is None or m.get("reps") == g["reps"])
    return {"type": m.get("kind") == g["kind"], "date": date_ok(g["d"], pd, ref), "value": v_ok and sr_ok}


def score(example, pred_items):
    ref = date.fromisoformat(example["ref"])
    gold = example["expected"]
    unused = list(range(len(pred_items)))
    per_gold = []
    for g in gold:
        best, best_i, best_s = None, None, -1
        for i in unused:
            p = pred_items[i]
            grp = "metric" if p.get("type") == "metric" else "action"
            if grp != g["t"]:
                continue
            c = item_correct(g, p, ref)
            s = sum(c.values())
            if s > best_s:
                best, best_i, best_s = c, i, s
        if best_i is not None:
            unused.remove(best_i)
        full = best is not None and all(best.values())
        kw_hit = None
        if g["t"] == "action" and best_i is not None:
            title = (pred_items[best_i].get("title") or "").lower()
            kw_hit = any(k in title for k in g["kw"]) if g["kw"] else None
        per_gold.append({"matched": best is not None, "checks": best, "correct": full, "kw": kw_hit})
    extras = len(unused)
    return {
        "gold_n": len(gold), "pred_n": len(pred_items), "extras": extras,
        "items": per_gold,
        "exact": all(x["correct"] for x in per_gold) and extras == 0,
        "correct_items": sum(x["correct"] for x in per_gold),
    }


# ------------------------------------------------------------------ sunucu
class Server:
    def __init__(self, model_file):
        args = [str(BIN), "-m", str(HERE / "models" / model_file), "-t", str(THREADS), "-c", str(CTX),
                "-ngl", "0", "-np", "1", "--host", "127.0.0.1", "--port", str(PORT), "--no-webui", "--reasoning", "off", *EXTRA]
        self.t0 = time.perf_counter()
        self.proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.ps = psutil.Process(self.proc.pid)
        self.peak_ws = self.peak_private = 0
        self.stop = False
        threading.Thread(target=self._watch, daemon=True).start()
        for _ in range(600):
            try:
                if requests.get(f"http://127.0.0.1:{PORT}/health", timeout=1).status_code == 200:
                    break
            except requests.RequestException:
                pass
            time.sleep(0.1)
        else:
            raise RuntimeError("server did not become healthy")
        self.load_s = time.perf_counter() - self.t0

    def _watch(self):
        while not self.stop:
            try:
                mi = self.ps.memory_info()
                self.peak_ws = max(self.peak_ws, mi.rss)
                self.peak_private = max(self.peak_private, getattr(mi, "private", mi.rss))
            except psutil.Error:
                return
            time.sleep(0.2)

    def close(self):
        self.stop = True
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def ask(ref, text):
    body = {"messages": messages(ref, text), "temperature": 0, "max_tokens": 400, "cache_prompt": True,
            "response_format": {"type": "json_schema", "json_schema": {"name": "extraction", "schema": SCHEMA}},
            "chat_template_kwargs": {"enable_thinking": False}}
    t = time.perf_counter()
    r = requests.post(f"http://127.0.0.1:{PORT}/v1/chat/completions", json=body, timeout=300)
    dt = time.perf_counter() - t
    r.raise_for_status()
    j = r.json()
    ask.prompt_tokens = j.get("usage", {}).get("prompt_tokens")
    return j["choices"][0]["message"]["content"], dt


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(q * (len(xs) - 1))))]


def run(key):
    data = [json.loads(l) for l in (HERE / "data" / f"{DATA}.jsonl").read_text(encoding="utf-8").splitlines()]
    srv = Server(MODELS[key])
    rows = []
    try:
        for ex in data:
            raw, dt = ask(ex["ref"], ex["text"])
            try:
                items = json.loads(raw).get("items", [])
                json_ok = True
            except json.JSONDecodeError:
                items, json_ok = [], False
            valid = [validate(i) for i in items]
            kept = [i for i, v in zip(items, valid) if v]
            sc = score(ex, kept)
            rows.append({"id": ex["id"], "lang": ex["lang"], "text": ex["text"], "raw": raw, "latency": dt,
                         "json_ok": json_ok, "prompt_tokens": ask.prompt_tokens, "all_valid": all(valid), "n_invalid": valid.count(False), "score": sc})
            print(f"{key} {ex['id']} {dt:5.2f}s exact={sc['exact']}", flush=True)
    finally:
        srv.close()
    (HERE / "results").mkdir(exist_ok=True)
    with (HERE / "results" / f"{key}.{PROMPT}{TAG}.jsonl").open("w", encoding="utf-8") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

    summ = {"model": key, "file": MODELS[key], "size_gb": round((HERE / "models" / MODELS[key]).stat().st_size / 1e9, 2),
            "load_s": round(srv.load_s, 2), "peak_ws_gb": round(srv.peak_ws / 1e9, 2), "peak_private_gb": round(srv.peak_private / 1e9, 2)}
    for lang in ("tr", "en", "all"):
        rs = [r for r in rows if lang == "all" or r["lang"] == lang]
        gold_items = [it for r in rs for it in r["score"]["items"]]
        acts = [it for it in gold_items if it["checks"] and "time" in it["checks"]]
        mets = [it for it in gold_items if it["checks"] and "value" in it["checks"]]
        n_gold = len(gold_items)
        n_pred = sum(r["score"]["pred_n"] for r in rs)
        n_corr = sum(it["correct"] for it in gold_items)
        lat = [r["latency"] for r in rs[1:]] if lang == "all" else [r["latency"] for r in rs]
        empty = [r for r in rs if r["score"]["gold_n"] == 0]
        kws = [it["kw"] for it in gold_items if it["kw"] is not None]
        summ[lang] = {
            "json_valid": round(sum(r["json_ok"] for r in rs) / len(rs), 3),
            "semantic_valid": round(sum(r["all_valid"] and r["json_ok"] for r in rs) / len(rs), 3),
            "field_accuracy": round(n_corr / n_gold, 3) if n_gold else None,
            "precision": round(n_corr / n_pred, 3) if n_pred else None,
            "exact_note": round(sum(r["score"]["exact"] for r in rs) / len(rs), 3),
            "type_match": round(sum(it["matched"] and it["checks"]["type"] for it in gold_items) / n_gold, 3),
            "date_acc": round(sum(it["checks"]["date"] for it in acts + mets) / max(1, len(acts + mets)), 3),
            "time_acc": round(sum(it["checks"]["time"] for it in acts) / max(1, len(acts)), 3),
            "metric_value_acc": round(sum(it["checks"]["value"] for it in mets) / max(1, len(mets)), 3),
            "none_acc": round(sum(r["score"]["pred_n"] == 0 for r in empty) / max(1, len(empty)), 3),
            "title_kw": round(sum(kws) / max(1, len(kws)), 3),
            "lat_p50": round(statistics.median(lat), 2), "lat_p95": round(pct(lat, 0.95), 2),
        }
    sp = HERE / "results" / "summary.json"
    allsum = json.loads(sp.read_text(encoding="utf-8")) if sp.exists() else {}
    summ["prompt"] = PROMPT
    summ["ctx"], summ["extra"], summ["max_prompt_tokens"] = CTX, " ".join(EXTRA), max(r.get("prompt_tokens") or 0 for r in rows)
    allsum[f"{key}.{PROMPT}{TAG}"] = summ
    sp.write_text(json.dumps(allsum, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(summ, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    for k in sys.argv[1:] or list(MODELS):
        run(k)
