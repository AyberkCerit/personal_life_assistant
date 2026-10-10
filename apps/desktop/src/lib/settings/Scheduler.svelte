<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import { api, type BackupStatus, type WeeklyReportView } from "../api";
  import { lang, t, tf } from "../i18n";
  import { WEEKDAYS, backupLine, describeError, reportLine } from "../settings";
  import Row from "./Row.svelte";
  import Banner from "../ui/Banner.svelte";
  import Button from "../ui/Button.svelte";

  let status = $state<BackupStatus | null>(null);
  // FR-SCH-016: the weekly report's day and time, or off
  let report = $state<WeeklyReportView | null>(null);
  let reportError = $state<string | null>(null);

  let savingReport = $state(false);

  /** One save at a time; a failed one puts the controls back (final review M5). */
  async function saveReport(change: Partial<WeeklyReportView["schedule"]>) {
    if (!report || savingReport) return;
    const before = report;
    report = { ...report, schedule: { ...report.schedule, ...change } };
    reportError = null;
    savingReport = true;
    try {
      await api.setWeeklyReport(report.schedule);
    } catch (e) {
      report = before;
      reportError = describeError(e);
    } finally {
      savingReport = false;
    }
  }
  let running = $state(false);
  let error = $state<string | null>(null);

  const when = (iso: string) => new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));

  async function refresh() {
    try {
      status = await api.backupStatus();
    } catch (e) {
      error = describeError(e);
    }
  }

  async function backupNow() {
    error = null;
    running = true;
    try {
      await api.backupNow(); // the result arrives as "backup-changed"
    } catch (e) {
      running = false;
      error = describeError(e);
    }
  }

  async function openFolder() {
    try {
      await api.openPlace("backups");
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => {
    void refresh();
    void api.weeklyReportSettings().then((r) => (report = r)).catch((e) => (reportError = describeError(e)));
    const un = api.onBackupChanged(() => {
      running = false;
      void refresh();
    });
    return () => void un.then((f) => f());
  });

  const line = $derived(status ? backupLine(status.job) : null);
</script>

<h4>{t("settings.report.title")}</h4>
<p class="hint">{t("settings.report.hint")}</p>
{#if report}
  {@const last = reportLine(report.job)}
  <Row label={t("settings.report.enabled")} forId="report-on">
    <input id="report-on" type="checkbox" disabled={savingReport} checked={report.schedule.enabled} onchange={(e) => void saveReport({ enabled: e.currentTarget.checked })} />
  </Row>
  {#if report.schedule.enabled}
    <Row label={t("settings.report.day")} forId="report-day">
      <select id="report-day" disabled={savingReport} value={report.schedule.weekday} onchange={(e) => void saveReport({ weekday: Number(e.currentTarget.value) })}>
        {#each WEEKDAYS as day, i (day)}<option value={i}>{t(day)}</option>{/each}
      </select>
    </Row>
    <Row label={t("settings.report.time")} forId="report-time">
      <input id="report-time" class="time" type="time" disabled={savingReport} value={report.schedule.time} onchange={(e) => e.currentTarget.value && void saveReport({ time: e.currentTarget.value })} />
    </Row>
  {/if}
  <p class:failed={last.key === "settings.report.failed"} class="status" role="status">{tf(last.key, { at: last.at ? when(last.at) : "", error: last.error ?? "" })}</p>
{/if}
{#if reportError}<Banner kind="danger" icon={CircleAlert} role="alert">{reportError}</Banner>{/if}

<h4>{t("settings.backup.title")}</h4>
<p class="hint">{t("settings.backup.hint")}</p>
{#if line}
  <p class:failed={line.key === "settings.backup.failed"} role="status">{tf(line.key, { at: line.at ? when(line.at) : "", error: line.error ?? "" })}</p>
  {#if status && status.backups.length > 0}<p class="hint">{tf("settings.backup.count", { count: status.backups.length })}</p>{/if}
{/if}
<div class="actions">
  <Button variant="primary" disabled={running} onclick={() => void backupNow()}>{running ? t("settings.backup.running") : t("settings.backup.now")}</Button>
  {#if status && status.backups.length > 0}<Button icon={FolderOpen} onclick={() => void openFolder()}>{t("settings.backup.open")}</Button>{/if}
</div>
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

<style>
  h4 { margin: var(--space-2) 0 var(--space-1); font-size: var(--text-md); font-weight: 600; }
  p { margin: 0 0 var(--space-2); font-size: var(--text-md); }
  .hint { color: var(--color-text-muted); font-size: var(--text-sm); }
  .failed { color: var(--color-danger); }
  .actions { display: flex; gap: var(--space-2); }
  .status { margin-top: var(--space-2); font-size: var(--text-sm); }
  .time {
    font: inherit; font-size: var(--text-md); color: var(--color-text); background: var(--color-surface-raised);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-md); padding: var(--space-1) var(--space-2);
  }
  h4:not(:first-child) { margin-top: var(--space-5); }
</style>
