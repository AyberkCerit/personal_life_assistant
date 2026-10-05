<script lang="ts" module>
  import type { MetricKind as Kind } from "../metrics";
  // The kind used last comes back next time: a daily habit is two keystrokes and Enter.
  let lastKind: Kind = "sleep";
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import X from "@lucide/svelte/icons/x";
  import { api, type MetricInput, type MetricRecord, type MetricUnit } from "../api";
  import { lang, t, tf, type Key } from "../i18n";
  import { KINDS, formatField, metricError, parseAmount, type MetricKind } from "../metrics";
  import { todayIso } from "../tasks";
  import { trapIndex } from "../settings";

  import { BASE, motion } from "../ui/motion";
  import Button from "../ui/Button.svelte";
  import IconButton from "../ui/IconButton.svelte";

  // FR-MET-003: kind → value → Enter. Also edits a record (FR-MET-011) when `record` is given.
  let { record = null, onClose, onSaved }: { record?: MetricRecord | null; onClose: () => void; onSaved: () => void } = $props();

  const UNITS: Record<MetricKind, MetricUnit[]> = { sleep: ["h", "min"], weight: ["kg", "lb"], steps: ["count"], water: ["glass", "ml", "l"], workout: ["kg", "lb"] };

  // svelte-ignore state_referenced_locally
  const start = record;
  let kind = $state<MetricKind>(start?.kind ?? lastKind);
  let amount = $state(start?.value != null ? String(Math.round(start.value * 100) / 100).replace(".", lang === "tr" ? "," : ".") : "");
  let unit = $state<MetricUnit>(start ? (UNITS[start.kind][0] === "glass" ? "ml" : UNITS[start.kind][0]) : UNITS[lastKind][0]);
  let date = $state(start?.date ?? todayIso());
  let exercise = $state(start?.exercise ?? "");
  let sets = $state(start?.sets != null ? String(start.sets) : "");
  let reps = $state(start?.reps != null ? String(start.reps) : "");
  let error = $state<string | null>(null);
  let askConfirm = $state<number | null>(null); // the out-of-range value awaiting a yes
  let busy = $state(false);
  let dialog: HTMLDivElement | undefined = $state();
  let valueInput: HTMLInputElement | undefined = $state();

  // A confirmation is for the value it was asked about: any change asks again (metrics final review I2).
  $effect(() => {
    void [kind, amount, unit, date, exercise, sets, reps];
    askConfirm = null;
  });

  function pick(k: MetricKind) {
    kind = k;
    unit = UNITS[k][0];
    error = null;
    askConfirm = null;
    void tick().then(() => (k === "workout" ? dialog?.querySelector<HTMLInputElement>("#qm-exercise") : valueInput)?.focus());
  }

  function input(confirmed: boolean): MetricInput {
    const whole = (s: string) => (s.trim() === "" ? null : Math.round(Number(s.trim())));
    return {
      kind,
      date,
      value: parseAmount(amount, kind),
      unit: kind === "steps" ? null : unit,
      exercise: kind === "workout" ? exercise.trim() || null : null,
      sets: kind === "workout" ? whole(sets) : null,
      reps: kind === "workout" ? whole(reps) : null,
      confirmed,
    };
  }

  async function save(confirmed = false) {
    const notWhole = (s: string) => s.trim() !== "" && !/^\d+$/.test(s.trim());
    if (kind === "workout" && (notWhole(sets) || notWhole(reps))) {
      error = t("metrics.error.not_number");
      return;
    }
    busy = true;
    error = null;
    try {
      if (start) await api.metricEdit(start.metric_id, input(confirmed));
      else await api.metricLog(input(confirmed));
      lastKind = kind;
      onSaved();
      onClose();
    } catch (e) {
      const { key, field, value, detail } = metricError(String(e));
      if (key === "metrics.error.out_of_range" && value !== null) {
        error = tf(key, { value: formatField(kind, field ?? "value", value, lang) });
        askConfirm = value; // after the reset above: this value is the one to confirm
      } else {
        error = tf(key, { detail: detail ?? "" });
      }
    } finally {
      busy = false;
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "Tab" && dialog) {
      const list = [...dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input, select")].filter((el) => el.tabIndex !== -1);
      e.preventDefault();
      list[trapIndex(list.indexOf(document.activeElement as HTMLElement), list.length, e.shiftKey)]?.focus();
    }
  }

  onMount(() => (kind === "workout" ? dialog?.querySelector<HTMLInputElement>("#qm-exercise") : valueInput)?.focus());
</script>

<svelte:window onkeydown={keydown} />
<div class="backdrop" transition:fade={{ duration: motion(BASE) }} onclick={onClose} aria-hidden="true"></div>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="qm-title" bind:this={dialog}>
  <header>
    <h2 id="qm-title">{start ? t("metrics.editTitle") : t("metrics.quick.title")}</h2>
    <IconButton icon={X} label={t("metrics.cancel")} onclick={onClose} />
  </header>
  <form onsubmit={(e) => { e.preventDefault(); void save(askConfirm !== null); }}>
    <div class="kinds" role="group" aria-label={t("metrics.title")}>
      {#each KINDS as k (k)}
        <button type="button" aria-pressed={k === kind} class:on={k === kind} disabled={!!start && k !== kind} onclick={() => pick(k)}>{t(`metrics.kind.${k}` as Key)}</button>
      {/each}
    </div>
    {#if kind === "workout"}
      <label>{t("metrics.exercise")}<input id="qm-exercise" bind:value={exercise} /></label>
      <div class="row">
        <label>{t("metrics.sets")}<input inputmode="numeric" bind:value={sets} /></label>
        <label>{t("metrics.reps")}<input inputmode="numeric" bind:value={reps} /></label>
        <label>{t("metrics.weightKg")}<input inputmode="decimal" bind:value={amount} /></label>
      </div>
    {:else}
      <div class="row">
        <label class="grow">{t("metrics.value")}<input bind:this={valueInput} inputmode="decimal" bind:value={amount} /></label>
        {#if UNITS[kind].length > 1}
          <label>{t("metrics.unit")}<select bind:value={unit}>{#each UNITS[kind] as u (u)}<option value={u}>{t(`metrics.unit.${u}` as Key)}</option>{/each}</select></label>
        {/if}
      </div>
    {/if}
    <label>{t("metrics.date")}<input type="date" bind:value={date} max={todayIso()} /></label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <footer>
      <Button variant="quiet" onclick={onClose}>{t("metrics.cancel")}</Button>
      <Button variant="primary" type="submit" disabled={busy}>{askConfirm !== null ? t("metrics.confirm") : t("metrics.save")}</Button>
    </footer>
  </form>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: var(--color-scrim); z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; top: 18vh; left: 50%; transform: translateX(-50%); width: min(26rem, calc(100vw - 2 * var(--space-4)));
    background: var(--color-surface-reading); border: 1px solid var(--color-border); border-radius: var(--radius-lg); box-shadow: var(--shadow-raised);
    padding: var(--space-3) var(--space-4) var(--space-4);
  }
  header { display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-2); }
  h2 { margin: 0; font-size: var(--text-lg); font-weight: 600; }
  form { display: flex; flex-direction: column; gap: var(--space-3); }
  .kinds { display: flex; flex-wrap: wrap; gap: var(--space-1); }
  .kinds button {
    padding: var(--space-1) var(--space-2); border: 1px solid var(--color-border-strong); border-radius: var(--radius-full);
    background: none; color: var(--color-text); font: inherit; font-size: var(--text-sm); cursor: pointer;
  }
  .kinds button.on { background: var(--color-accent-subtle); border-color: var(--color-accent); }
  .kinds button:disabled { opacity: 0.4; cursor: default; }
  .kinds button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  label { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); min-width: 0; }
  .row { display: flex; gap: var(--space-2); }
  .row label { flex: 1; }
  .row .grow { flex: 2; }
  input, select {
    font: inherit; font-size: var(--text-md); color: var(--color-text); background: var(--color-surface-raised);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-md); padding: var(--space-1) var(--space-2); min-width: 0;
  }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  .error { margin: 0; color: var(--color-danger); font-size: var(--text-sm); }
  footer { display: flex; justify-content: flex-end; gap: var(--space-2); }
</style>
