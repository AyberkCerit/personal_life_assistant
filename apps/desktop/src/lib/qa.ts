// The assistant panel (FR-QA-001…016): turns as the panel shows them, the events that build the
// live one, answer text with its [[links]], and what each tool did in the user's words.
import type { Key } from "./i18n";

export interface Undo { kind: string; id: string }
export interface ToolRecord {
  tool: string;
  args: Record<string, unknown>;
  ok: boolean;
  result: Record<string, unknown> | null;
  error: string | null;
  undo: Undo | null;
}
export interface QaTurn {
  turn_id: string;
  question: string;
  answer: string;
  tools: ToolRecord[];
  created_at: string;
  status: "done" | "stopped" | "failed" | "running";
  new_topic: boolean;
}
export type QaEvent =
  | { kind: "working"; turn_id: string; tool: string | null }
  | { kind: "tool"; turn_id: string; record: ToolRecord }
  | { kind: "token"; turn_id: string; text: string }
  | { kind: "done"; turn_id: string; turn: QaTurn; sources: string[] }
  | { kind: "failed"; turn_id: string; code: string; detail: string };

/** A turn on screen: the stored one plus what only the live answer has. */
export interface ShownTurn extends QaTurn {
  sources: string[];
  working: string | null | undefined; // undefined: not working; null: thinking; a tool's name
  error: { code: string; detail: string } | null;
}

export function started(turn_id: string, question: string, new_topic: boolean): ShownTurn {
  return { turn_id, question, answer: "", tools: [], created_at: "", status: "running", new_topic, sources: [], working: null, error: null };
}

export function shown(t: QaTurn): ShownTurn {
  return { ...t, sources: [], working: undefined, error: null };
}

/** The live turn after `e` (events of other turns change nothing). */
export function apply(turn: ShownTurn, e: QaEvent): ShownTurn {
  if (e.turn_id !== turn.turn_id) return turn;
  switch (e.kind) {
    case "working":
      return { ...turn, working: e.tool };
    case "tool":
      return { ...turn, tools: [...turn.tools, e.record] };
    case "token":
      return { ...turn, answer: turn.answer + e.text, working: undefined };
    case "done":
      return { ...shown(e.turn), sources: e.sources };
    case "failed":
      return { ...turn, status: "failed", working: undefined, error: { code: e.code, detail: e.detail } };
  }
}

/** Answer text cut into plain text and `[[link]]`s (the link's shown text is its alias, if any). */
export function answerParts(text: string): { text: string; link: string | null }[] {
  const parts: { text: string; link: string | null }[] = [];
  const re = /\[\[([^\]\n]+)\]\]/g;
  let at = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > at) parts.push({ text: text.slice(at, m.index), link: null });
    const [target, alias] = m[1].split("|");
    parts.push({ text: (alias ?? target).trim(), link: target.split("#")[0].trim() });
    at = m.index + m[0].length;
  }
  if (at < text.length) parts.push({ text: text.slice(at), link: null });
  return parts;
}

const s = (v: unknown) => (v === null || v === undefined ? "" : String(v));

/** What a tool call did, as an i18n key and its values (FR-QA-008: shown with Undo). */
export function toolLine(r: ToolRecord): { key: Key; values: Record<string, string> } {
  if (!r.ok) return { key: "qa.tool.failed", values: { tool: r.tool, reason: s(r.error) } };
  const res = r.result ?? {};
  const when = [s(res.date), s(res.time)].filter(Boolean).join(" ");
  switch (r.tool) {
    case "add_task":
      return { key: res.remind ? "qa.tool.reminder" : "qa.tool.task", values: { title: s(res.title), when } };
    case "complete_task":
      return { key: "qa.tool.completed", values: { title: s(res.title) } };
    case "log_metric":
      return { key: "qa.tool.metric", values: { kind: s(res.kind), value: s(res.value), unit: s(res.unit), date: s(res.date) } };
    case "create_note":
      return { key: "qa.tool.note", values: { title: s(res.title) } };
    case "search_notes":
      return { key: "qa.tool.searched", values: { query: s(r.args.query), n: s((res.notes as unknown[] | undefined)?.length ?? 0) } };
    case "query_tasks":
      return { key: "qa.tool.tasks", values: { n: s(res.count) } };
    default:
      return { key: "qa.tool.metrics", values: { kind: s(res.kind), days: s(r.args.days) } };
  }
}

/** A failed question's message key (FR-QA-002 and the rest). */
export function qaError(code: string): Key {
  if (code === "no_model") return "qa.error.no_model";
  if (code === "busy") return "qa.error.busy";
  if (code === "no_vault") return "qa.error.no_vault";
  return "qa.error.model";
}
