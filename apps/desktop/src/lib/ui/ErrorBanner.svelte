<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import { t, type Failure } from "../i18n";

  // SRS error table (M6 translations): what went wrong in the user's language, and the technical
  // text only behind "Copy details", to paste into a report.
  let { error }: { error: string | Failure } = $props();

  const text = $derived(typeof error === "string" ? error : error.text);
  const details = $derived(typeof error === "string" ? "" : error.details);
  let copied = $state(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(details);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      copied = false; // no clipboard: the text stays where it is
    }
  }
</script>

<Banner kind="danger" icon={CircleAlert} role="alert">
  {text}
  {#snippet actions()}
    {#if details}<Button variant="quiet" onclick={() => void copy()}>{copied ? t("error.copied") : t("error.copyDetails")}</Button>{/if}
  {/snippet}
</Banner>
