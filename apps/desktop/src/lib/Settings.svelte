<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import X from "@lucide/svelte/icons/x";
  import type { Lang } from "./i18n";
  import { t, type Key } from "./i18n";
  import { SECTIONS, trapIndex, type Section } from "./settings";
  import { BASE, motion } from "./ui/motion";
  import IconButton from "./ui/IconButton.svelte";
  import General from "./settings/General.svelte";
  import Vault from "./settings/Vault.svelte";
  import Ai from "./settings/Ai.svelte";
  import Scheduler from "./settings/Scheduler.svelte";
  import Notifications from "./settings/Notifications.svelte";
  import Privacy from "./settings/Privacy.svelte";
  import About from "./settings/About.svelte";

  // FR-SET-011: a window over the app (owner decision B). The section lives in App, so a language
  // switch (which re-mounts everything) keeps the same section open.
  let {
    section = $bindable(),
    refocus = $bindable(),
    onClose,
    onLanguage,
    onTheme,
    onSwitchVault,
  }: {
    section: Section;
    refocus: string | null;
    onClose: () => void;
    onLanguage: (l: Lang) => Promise<void>;
    onTheme: (theme: "dark" | "light") => Promise<void>;
    onSwitchVault: (path: string) => Promise<void>;
  } = $props();

  let dialog: HTMLDivElement | undefined = $state();
  // After a language switch the window is new; the section puts focus back where it was.
  const pendingFocus = refocus;

  function focusables(): HTMLElement[] {
    return dialog ? [...dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), a[href], [tabindex='0']")] : [];
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "Tab") {
      const list = focusables();
      if (list.length === 0) return;
      e.preventDefault();
      list[trapIndex(list.indexOf(document.activeElement as HTMLElement), list.length, e.shiftKey)].focus();
    }
  }

  function navKey(e: KeyboardEvent) {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    e.preventDefault();
    const next = SECTIONS[(SECTIONS.indexOf(section) + step + SECTIONS.length) % SECTIONS.length];
    section = next;
    dialog?.querySelector<HTMLElement>(`[data-section="${next}"]`)?.focus();
  }

  onMount(() => {
    if (!pendingFocus) dialog?.querySelector<HTMLElement>(`[data-section="${section}"]`)?.focus();
    refocus = null;
  });
</script>

<div class="backdrop" transition:fade={{ duration: motion(BASE) }} onclick={onClose} aria-hidden="true"></div>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" tabindex="-1" bind:this={dialog} onkeydown={keydown}>
  <nav aria-label={t("settings.title")}>
    <h2 id="settings-title">{t("settings.title")}</h2>
    <ul role="tablist" aria-orientation="vertical">
      {#each SECTIONS as s (s)}
        <li>
          <button
            role="tab"
            data-section={s}
            aria-selected={s === section}
            aria-controls="settings-panel"
            tabindex={s === section ? 0 : -1}
            class:current={s === section}
            onclick={() => (section = s)}
            onkeydown={navKey}>{t(`settings.section.${s}` as Key)}</button
          >
        </li>
      {/each}
    </ul>
  </nav>
  <div class="panel" id="settings-panel" role="tabpanel" aria-label={t(`settings.section.${section}` as Key)}>
    <header>
      <h3>{t(`settings.section.${section}` as Key)}</h3>
      <IconButton icon={X} label={t("settings.close")} onclick={onClose} />
    </header>
    <div class="body">
      {#if section === "general"}<General {onLanguage} {onTheme} focusLanguage={pendingFocus === "language"} />
      {:else if section === "vault"}<Vault {onSwitchVault} />
      {:else if section === "ai"}<Ai />
      {:else if section === "scheduler"}<Scheduler />
      {:else if section === "notifications"}<Notifications />
      {:else if section === "privacy"}<Privacy />
      {:else}<About />{/if}
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: var(--color-scrim); z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; inset: var(--space-8) max(var(--space-4), calc((100vw - 52rem) / 2));
    display: grid; grid-template-columns: 11rem 1fr; overflow: hidden;
    background: var(--color-surface-reading); border: 1px solid var(--color-border); border-radius: var(--radius-lg);
    box-shadow: var(--shadow-raised); outline: none;
  }
  nav { background: var(--color-surface-panel); border-right: 1px solid var(--color-border); padding: var(--space-3) var(--space-2); overflow-y: auto; }
  h2 { margin: 0 0 var(--space-3); padding: 0 var(--space-2); font-size: var(--text-lg); font-weight: 600; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  [role="tab"] {
    width: 100%; text-align: left; padding: var(--space-1) var(--space-2); border: 0; border-radius: var(--radius-md);
    background: none; color: var(--color-text-muted); font: inherit; font-size: var(--text-md); cursor: pointer;
  }
  [role="tab"]:hover { background: var(--color-surface-hover); color: var(--color-text); }
  [role="tab"].current { background: var(--color-accent-subtle); color: var(--color-text); }
  [role="tab"]:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .panel { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  header { display: flex; align-items: center; justify-content: space-between; padding: var(--space-3) var(--space-4) var(--space-2); }
  h3 { margin: 0; font-size: var(--text-xl); font-weight: 600; }
  .body { padding: 0 var(--space-4) var(--space-4); overflow-y: auto; }
</style>
