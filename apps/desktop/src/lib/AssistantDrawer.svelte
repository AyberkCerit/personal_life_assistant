<script lang="ts">
  import { onMount, tick } from "svelte";
  import { cubicIn, cubicOut } from "svelte/easing";
  import { fade, fly } from "svelte/transition";
  import Bot from "@lucide/svelte/icons/bot";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import FileText from "@lucide/svelte/icons/file-text";
  import Send from "@lucide/svelte/icons/send-horizontal";
  import Square from "@lucide/svelte/icons/square";
  import Trash from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import { api } from "./api";
  import { t, tf, type Key } from "./i18n";
  import { answerParts, apply, qaError, shown, started, toolLine, type QaEvent, type ShownTurn } from "./qa";
  import Button from "./ui/Button.svelte";
  import IconButton from "./ui/IconButton.svelte";
  import { motion } from "./ui/motion";

  // FR-QA-001…016, owner decision B: a wide drawer from the right over the editor. It stays mounted
  // while a vault is open, so an answer goes on (and is seen) when the drawer is closed and reopened.
  let {
    open,
    modelMissing,
    onClose,
    onOpenNote,
    onOpenLink,
    onShowModel,
  }: {
    open: boolean;
    modelMissing: boolean;
    onClose: () => void;
    onOpenNote: (path: string) => void;
    onOpenLink: (target: string) => void;
    onShowModel: () => void;
  } = $props();

  let turns = $state<ShownTurn[]>([]);
  let question = $state("");
  let newTopic = $state(false); // the next question starts a new topic
  let confirmClear = $state(false);
  let notice = $state<string | null>(null);
  let input: HTMLTextAreaElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  // Events can come before `qaAsk` returns the turn id (a quick failure): kept until it does (I7).
  let early: QaEvent[] = [];

  const live = $derived(turns.find((x) => x.status === "running") ?? null);

  async function scrollDown() {
    await tick();
    list?.scrollTo({ top: list.scrollHeight });
  }

  async function load() {
    turns = (await api.qaHistory().catch(() => [])).map(shown);
    void scrollDown();
  }

  async function send() {
    const q = question.trim();
    if (!q || live) return;
    notice = null;
    try {
      const id = await api.qaAsk(q, newTopic);
      let turn = started(id, q, newTopic);
      for (const e of early.filter((x) => x.turn_id === id)) turn = apply(turn, e);
      early = [];
      turns = [...turns, turn];
      question = "";
      newTopic = false;
      void scrollDown();
    } catch (e) {
      notice = t(qaError(String(e).split("|")[0]));
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  async function undo(turn: ShownTurn, index: number) {
    try {
      await api.qaUndo(turn.turn_id, index);
      turns = turns.map((x) =>
        x.turn_id !== turn.turn_id ? x : { ...x, tools: x.tools.map((r, i) => (i === index ? { ...r, undo: null, result: { ...r.result, undone: true } } : r)) },
      );
    } catch (e) {
      notice = tf("qa.error.undo", { reason: String(e).split("|").pop() || String(e) });
    }
  }

  async function clear() {
    confirmClear = false;
    try {
      await api.qaClear();
      turns = [];
    } catch (e) {
      notice = t(qaError(String(e).split("|")[0]));
    }
  }

  function startTopic() {
    newTopic = true;
    input?.focus();
  }

  function dialogKey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    e.preventDefault();
    if (confirmClear) confirmClear = false;
    else onClose();
  }

  const name = (path: string) => path.split("/").pop()!.replace(/\.md$/i, "");
  const failure = (turn: ShownTurn) => (turn.error ? tf(qaError(turn.error.code), { detail: turn.error.detail }) : "");

  $effect(() => {
    if (open) void tick().then(() => input?.focus());
  });

  onMount(() => {
    void load();
    const un = api.onQaEvent((e) => {
      if (!turns.some((x) => x.turn_id === e.turn_id)) early = [...early.slice(-50), e];
      turns = turns.map((x) => apply(x, e));
      if (e.kind === "token" || e.kind === "tool" || e.kind === "done") void scrollDown();
    });
    return () => void un.then((f) => f());
  });
</script>

{#if open}
  <div class="scrim" aria-hidden="true" onclick={onClose} transition:fade={{ duration: motion(180) }}></div>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="drawer"
    role="dialog"
    aria-modal="true"
    aria-labelledby="qa-title"
    tabindex="-1"
    onkeydown={dialogKey}
    in:fly={{ x: 520, duration: motion(220), easing: cubicOut, opacity: 1 }}
    out:fly={{ x: 520, duration: motion(160), easing: cubicIn, opacity: 1 }}
  >
    <header>
      <h2 id="qa-title"><Bot size={18} strokeWidth={1.75} aria-hidden="true" />{t("qa.title")}</h2>
      <div class="actions">
        <Button variant="quiet" onclick={startTopic} disabled={newTopic}>{t("qa.newTopic")}</Button>
        <IconButton icon={Trash} label={t("qa.clear")} onclick={() => (confirmClear = true)} disabled={turns.length === 0 || live !== null} />
        <IconButton icon={X} label={t("qa.close")} onclick={onClose} />
      </div>
    </header>
    {#if confirmClear}
      <div class="confirm" role="alertdialog" aria-label={t("qa.clear")}>
        <span>{t("qa.clearConfirm")}</span>
        <Button variant="quiet" onclick={() => (confirmClear = false)}>{t("qa.cancel")}</Button>
        <Button variant="danger" onclick={() => void clear()}>{t("qa.clearYes")}</Button>
      </div>
    {/if}

    <div class="turns" bind:this={list} aria-live="polite">
      {#if turns.length === 0}
        <div class="empty">
          <Bot size={28} strokeWidth={1.5} aria-hidden="true" />
          <h3>{t("qa.emptyTitle")}</h3>
          <p>{t("qa.emptyBody")}</p>
        </div>
      {/if}
      {#each turns as turn, n (turn.turn_id)}
        {#if turn.new_topic && n > 0}<div class="topic" role="separator">{t("qa.topicStart")}</div>{/if}
        <article class="turn">
          <p class="question">{turn.question}</p>
          {#each turn.tools as record, i (i)}
            {@const line = toolLine(record)}
            <div class="tool" class:failed={!record.ok}>
              <span>{tf(line.key, line.values)}</span>
              {#if record.result?.undone}
                <span class="undone">{t("qa.undone")}</span>
              {:else if record.undo && turn.status !== "running"}
                <Button variant="quiet" onclick={() => void undo(turn, i)}>{t("qa.undo")}</Button>
              {/if}
            </div>
          {/each}
          {#if turn.answer}
            <p class="answer">
              {#each answerParts(turn.answer) as part, i (i)}
                {#if part.link}<button class="link" onclick={() => onOpenLink(part.link!)}>{part.text}</button>{:else}{part.text}{/if}
              {/each}
            </p>
          {/if}
          {#if turn.status === "running" && turn.working !== undefined}
            <p class="working">{turn.working ? t(`qa.using.${turn.working}` as Key) : t("qa.thinking")}</p>
          {/if}
          {#if turn.status === "stopped"}<p class="muted">{t("qa.stopped")}</p>{/if}
          {#if turn.status === "failed"}<p class="error"><CircleAlert size={14} strokeWidth={2} aria-hidden="true" />{failure(turn)}</p>{/if}
          {#if turn.sources.length}
            <div class="sources">
              <span>{t("qa.sources")}</span>
              {#each turn.sources as path (path)}
                <button class="source" title={path} onclick={() => onOpenNote(path)}><FileText size={12} strokeWidth={2} aria-hidden="true" />{name(path)}</button>
              {/each}
            </div>
          {/if}
        </article>
      {/each}
      {#if newTopic && turns.length}<div class="topic" role="separator">{t("qa.topicStart")}</div>{/if}
    </div>

    <footer>
      {#if modelMissing}
        <div class="nomodel">
          <span>{t("qa.error.no_model")}</span>
          <Button variant="primary" onclick={onShowModel}>{t("qa.download")}</Button>
        </div>
      {/if}
      {#if notice}<p class="error" role="alert"><CircleAlert size={14} strokeWidth={2} aria-hidden="true" />{notice}</p>{/if}
      <div class="ask">
        <textarea
          bind:this={input}
          bind:value={question}
          rows="2"
          maxlength="2000"
          placeholder={t("qa.placeholder")}
          aria-label={t("qa.placeholder")}
          onkeydown={keydown}
          disabled={modelMissing}
        ></textarea>
        {#if live}
          <Button icon={Square} onclick={() => void api.qaStop()}>{t("qa.stop")}</Button>
        {:else}
          <Button variant="primary" icon={Send} onclick={() => void send()} disabled={!question.trim() || modelMissing}>{t("qa.send")}</Button>
        {/if}
      </div>
    </footer>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: var(--color-scrim); z-index: 30; }
  .drawer {
    position: fixed; top: 0; right: 0; bottom: 0; z-index: 31; width: min(520px, calc(100vw - 4 * var(--space-8)));
    display: flex; flex-direction: column; background: var(--color-surface-reading);
    border-left: 1px solid var(--color-border); box-shadow: var(--shadow-raised); outline: none;
  }
  header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-3) var(--space-4); border-bottom: 1px solid var(--color-border); }
  h2 { display: flex; align-items: center; gap: var(--space-2); margin: 0; font-size: var(--text-lg); font-weight: 600; }
  .actions { display: flex; align-items: center; gap: var(--space-1); }
  .confirm {
    display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-4);
    background: var(--color-danger-subtle); font-size: var(--text-sm);
  }
  .confirm span { flex: 1 1 12rem; }
  .turns { flex: 1; overflow-y: auto; padding: var(--space-4); display: flex; flex-direction: column; gap: var(--space-4); }
  .empty { margin: auto; max-width: 22rem; text-align: center; color: var(--color-text-muted); display: flex; flex-direction: column; align-items: center; gap: var(--space-2); }
  .empty h3 { margin: 0; color: var(--color-text); font-size: var(--text-md); }
  .empty p { margin: 0; font-size: var(--text-sm); }
  .turn { display: flex; flex-direction: column; gap: var(--space-2); }
  .question {
    align-self: flex-end; max-width: 85%; margin: 0; padding: var(--space-2) var(--space-3); white-space: pre-wrap;
    background: var(--color-accent-subtle); border-radius: var(--radius-lg); font-size: var(--text-md);
  }
  .answer { margin: 0; white-space: pre-wrap; line-height: 1.6; font-size: var(--text-md); }
  .link { border: 0; padding: 0; background: none; font: inherit; color: var(--color-link); text-decoration: underline; text-decoration-color: var(--color-link-underline); text-underline-offset: 3px; cursor: pointer; }
  .tool {
    display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    border-left: 2px solid var(--color-accent); background: var(--color-surface-panel); border-radius: var(--radius-sm); font-size: var(--text-sm);
  }
  .tool.failed { border-left-color: var(--color-danger); }
  .undone { color: var(--color-text-muted); }
  .working, .muted { margin: 0; color: var(--color-text-muted); font-size: var(--text-sm); }
  .working { animation: pulse 1.4s ease-in-out infinite; }
  @media (prefers-reduced-motion: reduce) { .working { animation: none; } }
  @keyframes pulse { 50% { opacity: 0.5; } }
  .error { display: flex; align-items: center; gap: var(--space-1); margin: 0; color: var(--color-danger); font-size: var(--text-sm); }
  .sources { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); }
  .source {
    display: inline-flex; align-items: center; gap: 2px; padding: 1px var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius-sm);
    background: var(--color-surface-panel); color: var(--color-link); font: inherit; cursor: pointer;
  }
  .topic { display: flex; align-items: center; gap: var(--space-2); color: var(--color-text-muted); font-size: var(--text-xs); }
  .topic::before, .topic::after { content: ""; flex: 1; border-top: 1px solid var(--color-border); }
  footer { border-top: 1px solid var(--color-border); padding: var(--space-3) var(--space-4); display: flex; flex-direction: column; gap: var(--space-2); }
  .nomodel { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); font-size: var(--text-sm); color: var(--color-warning); }
  .ask { display: flex; align-items: flex-end; gap: var(--space-2); }
  textarea {
    flex: 1; resize: none; max-height: 10rem; font: inherit; font-size: var(--text-md); color: var(--color-text);
    background: var(--color-surface-raised); border: 1px solid var(--color-border-strong); border-radius: var(--radius-md); padding: var(--space-2);
  }
  textarea:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -1px; }
</style>
