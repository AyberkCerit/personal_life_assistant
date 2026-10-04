<script lang="ts">
  import { onMount } from "svelte";
  import { api, type About } from "../api";
  import { t } from "../i18n";
  import Logo from "../ui/Logo.svelte";

  // FR-SET-017: version, active model, bundled components and their licences.
  let about = $state<About | null>(null);
  onMount(() => void api.aboutInfo().then((a) => (about = a)));
</script>

{#if about}
  <div class="head">
    <Logo size={40} />
    <dl>
      <dt>{t("settings.about.version")}</dt><dd>PLA {about.version}</dd>
      <dt>{t("settings.about.model")}</dt><dd>{about.model ?? t("settings.about.noModel")}{about.model_licence ? ` · ${about.model_licence}` : ""}</dd>
    </dl>
  </div>
  <h4>{t("settings.about.components")}</h4>
  <table>
    <tbody>
      {#each about.components as c (c.name)}
        <tr><td><a href={c.url} target="_blank" rel="noreferrer">{c.name}</a></td><td>{c.licence}</td></tr>
      {/each}
    </tbody>
  </table>
  <p class="hint">{t("settings.about.more")}</p>
{/if}

<style>
  .head { display: flex; gap: var(--space-4); align-items: center; padding: var(--space-2) 0 var(--space-3); border-bottom: 1px solid var(--color-border); }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: var(--space-1) var(--space-3); margin: 0; font-size: var(--text-md); }
  dt { color: var(--color-text-muted); }
  dd { margin: 0; }
  h4 { margin: var(--space-3) 0 var(--space-1); font-size: var(--text-md); font-weight: 600; }
  table { border-collapse: collapse; width: 100%; font-size: var(--text-sm); }
  td { padding: var(--space-1) 0; border-bottom: 1px solid var(--color-border); }
  td:last-child { color: var(--color-text-muted); text-align: right; }
  a { color: var(--color-link); }
  .hint { margin: var(--space-2) 0 0; color: var(--color-text-muted); font-size: var(--text-sm); }
</style>
