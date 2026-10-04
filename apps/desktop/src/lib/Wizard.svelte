<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Cloud from "@lucide/svelte/icons/cloud";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Info from "@lucide/svelte/icons/info";
  import { api, inTauri, type DownloadState, type ModelStatus, type VaultInfo } from "./api";
  import { lang, t, tf, type Lang } from "./i18n";
  import { folderAdvice, nextStep, prevStep, setupError, STEPS, type Advice, type Progress, type Step } from "./wizard";
  import ModelCard from "./ModelCard.svelte";
  import Banner from "./ui/Banner.svelte";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";
  import Logo from "./ui/Logo.svelte";

  // Progress lives in App so a language switch (which re-mounts this component) keeps it.
  let { progress = $bindable(), onLang, onFinish }: { progress: Progress; onLang: (l: Lang) => void; onFinish: (v: VaultInfo) => void } = $props();

  let heading: HTMLHeadingElement | undefined = $state();
  // The advice and the path it was worked out for; while typing it is stale and hidden, and the
  // backend still refuses a bad folder when the button is pressed.
  let checked = $state<{ path: string; advice: Advice[] }>({ path: "", advice: [] });
  const advice = $derived(checked.path === progress.path.trim() ? checked.advice : []);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let model = $state<ModelStatus | null>(null);
  let download = $state<DownloadState | null>(null);

  const usable = $derived(progress.path.trim() !== "" && advice.every((a) => a.usable));
  const modelState = $derived(model?.installed ? "ready" : download && download.state !== "failed" ? "downloading" : "off");

  async function go(step: Step) {
    error = null;
    progress.step = step;
    if (step === "model" || step === "done") await refreshModel();
    await tick();
    heading?.focus(); // screen readers hear the new step
  }

  async function refreshModel() {
    model = await api.modelStatus();
    download = model.download;
  }

  async function chooseLanguage(l: Lang) {
    await api.setLanguage(l);
    onLang(l); // re-mounts the app in the new language
  }

  async function browse() {
    const picked = inTauri ? await open({ directory: true, defaultPath: progress.path || undefined }) : "C:/Notlar";
    if (typeof picked === "string") progress.path = picked;
  }

  async function useFolder() {
    busy = true;
    error = null;
    try {
      progress.opened = await api.setupVault(progress.path.trim(), lang);
      await go(nextStep("vault"));
    } catch (e) {
      const { key, path, reason } = setupError(String(e));
      error = tf(key, { path, reason });
    } finally {
      busy = false;
    }
  }

  async function finish() {
    if (!progress.opened) return;
    busy = true;
    try {
      await api.finishSetup();
      onFinish(progress.opened);
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  // The vault step says what choosing the typed folder will do, a moment after typing stops.
  $effect(() => {
    const path = progress.path.trim();
    if (progress.step !== "vault") return;
    untrack(() => (error = null)); // an error was about the previous path
    if (path === "") return;
    const timer = setTimeout(() => {
      void api.inspectVaultFolder(path).then((r) => (checked = { path, advice: folderAdvice(r) }));
    }, 200);
    return () => clearTimeout(timer);
  });

  onMount(() => {
    if (progress.path === "") void api.wizardDefaults().then((d) => (progress.path ||= d.suggested_vault ?? ""));
    if (progress.step === "model" || progress.step === "done") void refreshModel();
    const un = api.onModelDownload((s) => (download = s));
    const unChanged = api.onModelChanged(() => void refreshModel());
    return () => {
      void un.then((f) => f());
      void unChanged.then((f) => f());
    };
  });

  const adviceIcon = (a: Advice) => (a.tone === "danger" ? CircleAlert : a.tone === "warning" ? Cloud : Info);
  const adviceKind = (a: Advice) => (a.tone === "info" ? "reminder" : a.tone);
</script>

<main class="wizard">
  <header>
    <Logo size={40} />
    <ol class="dots" aria-label={tf("wizard.step", { n: STEPS.indexOf(progress.step) + 1 })}>
      {#each STEPS as s, i (s)}
        <li class:current={s === progress.step} class:past={i < STEPS.indexOf(progress.step)}></li>
      {/each}
    </ol>
    <p class="step">{tf("wizard.step", { n: STEPS.indexOf(progress.step) + 1 })}</p>
  </header>

  {#if progress.step === "language"}
    <h1 bind:this={heading} tabindex="-1">{t("wizard.language.title")}</h1>
    <p class="body">{t("wizard.language.body")}</p>
    <div class="choices" role="radiogroup" aria-label={t("wizard.language.title")}>
      {#each [["tr", "Türkçe"], ["en", "English"]] as [code, name] (code)}
        <label class="choice" class:selected={lang === code}>
          <input type="radio" name="lang" value={code} checked={lang === code} onchange={() => void chooseLanguage(code as Lang)} />
          <span>{name}</span>
        </label>
      {/each}
    </div>
  {:else if progress.step === "vault"}
    <h1 bind:this={heading} tabindex="-1">{t("wizard.vault.title")}</h1>
    <p class="body">{t("wizard.vault.body")}</p>
    <label class="field">
      <span>{t("wizard.vault.path")}</span>
      <span class="row">
        <input type="text" bind:value={progress.path} spellcheck="false" onkeydown={(e) => e.key === "Enter" && usable && !busy && void useFolder()} />
        <Button icon={FolderOpen} onclick={() => void browse()}>{t("wizard.vault.browse")}</Button>
      </span>
    </label>
    <div class="advice" aria-live="polite">
      {#each advice as a (a.key)}
        <Banner kind={adviceKind(a)} icon={adviceIcon(a)}>{tf(a.key, { count: a.count ?? 0 })}</Banner>
      {/each}
    </div>
  {:else if progress.step === "model"}
    <h1 bind:this={heading} tabindex="-1">{t("wizard.model.title")}</h1>
    <p class="body">{t("wizard.model.body")}</p>
    {#if model?.installed}
      <p class="ok"><Icon icon={Check} /> {t("wizard.model.installed")}: {model.installed.name}</p>
    {:else if model}
      <ModelCard status={model} onDone={() => void refreshModel()} />
      <p class="hint">{t("wizard.model.later")}</p>
    {/if}
  {:else}
    <h1 bind:this={heading} tabindex="-1">{t("wizard.done.title")}</h1>
    <dl>
      <dt>{t("wizard.done.vault")}</dt><dd class="path">{progress.opened?.path}</dd>
      <dt>{t("wizard.done.model")}</dt>
      <dd>{t(modelState === "ready" ? "wizard.done.modelReady" : modelState === "downloading" ? "wizard.done.modelDownloading" : "wizard.done.modelOff")}</dd>
    </dl>
  {/if}

  {#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

  <footer>
    {#if progress.step !== "language"}
      <Button variant="quiet" icon={ArrowLeft} disabled={busy} onclick={() => void go(prevStep(progress.step))}>{t("wizard.back")}</Button>
    {/if}
    <span class="spacer"></span>
    {#if progress.step === "language"}
      <Button variant="primary" onclick={() => void chooseLanguage(lang).then(() => go("vault"))}>{t("wizard.next")}</Button>
    {:else if progress.step === "vault"}
      <Button variant="primary" disabled={!usable || busy} onclick={() => void useFolder()}>{t("wizard.vault.use")}</Button>
    {:else if progress.step === "model"}
      {#if modelState === "off"}
        <Button onclick={() => void go("done")}>{t("wizard.model.skip")}</Button>
      {:else}
        <Button variant="primary" onclick={() => void go("done")}>{t("wizard.next")}</Button>
      {/if}
    {:else}
      <Button variant="primary" icon={Check} disabled={busy} onclick={() => void finish()}>{t("wizard.done.start")}</Button>
    {/if}
  </footer>
</main>

<style>
  .wizard {
    margin: auto; width: min(34rem, 100%); padding: var(--space-8) var(--space-4);
    display: flex; flex-direction: column; gap: var(--space-3);
  }
  header { display: flex; align-items: center; gap: var(--space-3); margin-bottom: var(--space-2); }
  .dots { display: flex; gap: var(--space-1); list-style: none; margin: 0; padding: 0; }
  .dots li { width: 1.5rem; height: 0.25rem; border-radius: var(--radius-full); background: var(--color-border-strong); }
  .dots li.past { background: var(--color-accent); }
  .dots li.current { width: 2.5rem; background: var(--color-accent); }
  .step { margin: 0 0 0 auto; color: var(--color-text-muted); font-size: var(--text-sm); }
  h1 { margin: 0; font-size: var(--text-2xl); font-weight: 600; outline: none; }
  p { margin: 0; }
  .body { color: var(--color-text-muted); }
  .choices { display: flex; gap: var(--space-2); }
  .choice {
    flex: 1; display: flex; align-items: center; gap: var(--space-2); padding: var(--space-3);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-lg); cursor: pointer;
  }
  .choice:hover { background: var(--color-surface-hover); }
  .choice.selected { border-color: var(--color-accent); background: var(--color-accent-subtle); }
  .choice input { accent-color: var(--color-accent); margin: 0; }
  .field { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); }
  .row { display: flex; gap: var(--space-2); }
  .row input {
    flex: 1; min-width: 0; font: inherit; font-size: var(--text-md); color: var(--color-text);
    background: var(--color-surface-raised); border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-md); padding: var(--space-1) var(--space-2);
  }
  .row input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  .advice { display: flex; flex-direction: column; }
  .advice :global(.banner), .wizard > :global(.banner) { margin: 0 0 var(--space-2); }
  .ok { display: flex; align-items: center; gap: var(--space-2); color: var(--color-accent); }
  .hint { color: var(--color-text-muted); font-size: var(--text-sm); }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: var(--space-2) var(--space-4); margin: 0; }
  dt { color: var(--color-text-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .path { font-family: var(--font-mono); font-size: var(--text-sm); }
  footer { display: flex; align-items: center; gap: var(--space-2); margin-top: var(--space-4); }
  .spacer { flex: 1; }
</style>
