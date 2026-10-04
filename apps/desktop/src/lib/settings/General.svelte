<script lang="ts">
  import { onMount, tick } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import { api, type SettingsView } from "../api";
  import { lang, t, type Lang } from "../i18n";
  import { describeError } from "../settings";
  import Banner from "../ui/Banner.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Row from "./Row.svelte";

  let {
    onLanguage,
    onTheme,
    focusLanguage = false,
  }: { onLanguage: (l: Lang) => Promise<void>; onTheme: (theme: "dark" | "light") => Promise<void>; focusLanguage?: boolean } = $props();
  let languageSelect: HTMLSelectElement | undefined = $state();

  let view = $state<SettingsView | null>(null);
  let error = $state<string | null>(null);
  let revision = $state(0); // bumped after a refused change: the controls are drawn again from what is saved

  async function run(action: () => Promise<void>) {
    error = null;
    try {
      await action();
    } catch (e) {
      error = describeError(e);
      view = await api.settingsGet(); // the controls show what is really saved (settings final review M5)
      revision++;
    }
  }

  onMount(() => {
    void api.settingsGet().then(async (v) => {
      view = v;
      await tick();
      if (focusLanguage) languageSelect?.focus();
    });
    const un = api.onAutostartChanged((on) => view && (view = { ...view, autostart: on }));
    return () => void un.then((f) => f());
  });
</script>

{#if view}
  {#key revision}
  <Row label={t("settings.language")} hint={t("settings.language.hint")} forId="set-language">
    <select id="set-language" bind:this={languageSelect} value={lang} onchange={(e) => void run(() => onLanguage(e.currentTarget.value as Lang))}>
      <option value="tr">Türkçe</option>
      <option value="en">English</option>
    </select>
  </Row>
  <Row label={t("settings.theme")} forId="set-theme">
    <select id="set-theme" value={view.theme} onchange={(e) => void run(() => onTheme(e.currentTarget.value as "dark" | "light"))}>
      <option value="dark">{t("settings.theme.dark")}</option>
      <option value="light">{t("settings.theme.light")}</option>
    </select>
  </Row>
  <Row label={t("settings.autostart")} hint={t("settings.autostart.hint")}>
    <Checkbox checked={view.autostart} label={t("settings.autostart")} hideLabel onchange={(on) => void run(() => api.setAutostart(on))} />
  </Row>
  {/key}
{/if}
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}
