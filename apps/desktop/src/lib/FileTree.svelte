<script lang="ts">
  import { tick } from "svelte";
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import type { TreeEntry } from "./api";
  import { t, tf } from "./i18n";
  import { moveTargets, parentOf, type TreeActions } from "./tree";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";

  // FR-EDT-004: the folder tree with create, rename, move and delete (owner decision: right-click
  // menu, F2/Del, drag and drop).
  let {
    entries,
    selected,
    inbox,
    onOpen,
    actions,
  }: { entries: TreeEntry[]; selected: string | null; inbox: string; onOpen: (path: string) => void; actions: TreeActions } = $props();

  type Dialog =
    | { kind: "move"; entry: TreeEntry; choice: string }
    | { kind: "delete"; entry: TreeEntry }
    | { kind: "links"; entry: TreeEntry; name: string; count: number }
    | { kind: "template"; folder: string; templates: string[]; choice: string; title: string };

  let title = $state("");
  let focused = $state<string | null>(null); // the row the keyboard and the menu act on
  let renaming = $state<{ path: string; value: string } | null>(null);
  let creating = $state<{ parent: string; kind: "note" | "folder"; value: string } | null>(null);
  let menu = $state<{ x: number; y: number; entry: TreeEntry | null } | null>(null);
  let dialog = $state<Dialog | null>(null);
  let dragging = $state<string | null>(null);
  let dropOn = $state<string | null>(null);
  let list: HTMLUListElement | undefined = $state();

  const byPath = $derived(new Map(entries.map((e) => [e.path, e])));
  const stem = (name: string) => name.replace(/\.md$/i, "");
  /** New notes go into the focused folder (or the focused note's folder), else the inbox (FR-VLT-009). */
  const target = $derived.by(() => {
    const e = focused ? byPath.get(focused) : undefined;
    if (!e) return inbox;
    return e.is_dir ? e.path : parentOf(e.path) || inbox;
  });

  // A dialog takes the focus, so Esc, Tab and Enter work in it: the name field, else the main button.
  let dialogEl: HTMLDivElement | undefined = $state();
  $effect(() => {
    if (!dialogEl) return;
    (dialogEl.querySelector<HTMLElement>(".field input") ?? dialogEl.querySelector<HTMLElement>("footer button:last-child"))?.focus();
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const name = title.trim();
    if (!name) return;
    await actions.createNote(target, name, null);
    title = "";
  }

  function rows(): HTMLElement[] {
    return list ? [...list.querySelectorAll<HTMLElement>("[data-row]")] : [];
  }

  async function focusRow(path: string | null) {
    focused = path;
    await tick();
    rows().find((r) => r.dataset.row === path)?.focus();
  }

  function keydown(e: KeyboardEvent) {
    if (renaming || creating) return;
    const all = rows();
    const at = all.findIndex((r) => r.dataset.row === focused);
    const entry = focused ? byPath.get(focused) : undefined;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = all[Math.min(Math.max(at + (e.key === "ArrowDown" ? 1 : -1), 0), all.length - 1)];
      void focusRow(next?.dataset.row ?? null);
    } else if (e.key === "F2" && entry) {
      e.preventDefault();
      startRename(entry);
    } else if (e.key === "Delete" && entry) {
      e.preventDefault();
      dialog = { kind: "delete", entry };
    } else if ((e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) && entry) {
      e.preventDefault();
      const box = all[at]?.getBoundingClientRect();
      menu = { x: (box?.left ?? 0) + 24, y: box?.bottom ?? 0, entry };
    }
  }

  function startRename(entry: TreeEntry) {
    menu = null;
    renaming = { path: entry.path, value: entry.is_dir ? entry.name : stem(entry.name) };
    void tick().then(() => list?.querySelector<HTMLInputElement>(".rename")?.select());
  }

  async function commitRename() {
    if (!renaming) return;
    const { path, value } = renaming;
    const entry = byPath.get(path);
    renaming = null;
    const name = value.trim();
    if (!entry || !name || name === (entry.is_dir ? entry.name : stem(entry.name))) return;
    if (entry.is_dir) {
      await actions.renameFolder(path, name);
      return;
    }
    const count = await actions.linkCount(path).catch(() => 0);
    if (count > 0) dialog = { kind: "links", entry, name, count }; // FR-EDT-013: ask first
    else await actions.renameNote(path, name, false);
  }

  function startCreate(parent: string, kind: "note" | "folder") {
    menu = null;
    creating = { parent, kind, value: "" };
    void tick().then(() => list?.querySelector<HTMLInputElement>(".create")?.focus());
  }

  async function commitCreate() {
    if (!creating) return;
    const { parent, kind, value } = creating;
    creating = null;
    const name = value.trim();
    if (!name) return;
    if (kind === "note") await actions.createNote(parent || inbox, name, null);
    else await actions.createFolder(parent, name);
  }

  async function openTemplates(folder: string) {
    menu = null;
    const templates = await actions.templates().catch(() => []);
    dialog = { kind: "template", folder: folder || inbox, templates, choice: templates[0] ?? "", title: "" };
  }

  function openMenu(e: MouseEvent, entry: TreeEntry | null) {
    e.preventDefault();
    if (entry) focused = entry.path;
    menu = { x: e.clientX, y: e.clientY, entry };
    void tick().then(() => document.querySelector<HTMLElement>(".tree-menu [role=menuitem]")?.focus());
  }

  function menuKey(e: KeyboardEvent) {
    const items = [...document.querySelectorAll<HTMLElement>(".tree-menu [role=menuitem]")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") {
      e.preventDefault();
      menu = null;
      void focusRow(focused);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      items[(at + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  }

  function folderOf(entry: TreeEntry | null): string {
    if (!entry) return "";
    return entry.is_dir ? entry.path : parentOf(entry.path);
  }

  // drag and drop: a note or folder onto a folder (or the empty area = the vault root)
  function dragStart(e: DragEvent, entry: TreeEntry) {
    dragging = entry.path;
    e.dataTransfer?.setData("text/plain", entry.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function canDrop(folder: string): boolean {
    return dragging !== null && moveTargets(entries, dragging).includes(folder);
  }

  function dragOver(e: DragEvent, folder: string) {
    if (!canDrop(folder)) return;
    e.preventDefault();
    dropOn = folder;
  }

  async function drop(e: DragEvent, folder: string) {
    e.preventDefault();
    const path = dragging;
    dragging = dropOn = null;
    if (path && moveTargets(entries, path).includes(folder)) await actions.move(path, folder);
  }

  async function confirm() {
    const d = dialog;
    dialog = null;
    if (!d) return;
    if (d.kind === "move") await actions.move(d.entry.path, d.choice);
    else if (d.kind === "delete") await actions.remove(d.entry.path);
    else if (d.kind === "template" && d.title.trim() && d.choice) await actions.createNote(d.folder, d.title.trim(), d.choice);
  }

  function ask(kind: "move" | "delete", entry: TreeEntry) {
    dialog = kind === "move" ? { kind, entry, choice: moveTargets(entries, entry.path)[0] ?? "" } : { kind, entry };
    menu = null;
  }

  async function renameWithLinks(update: boolean) {
    const d = dialog; // read before closing: a template {@const} would follow `dialog` to null
    dialog = null;
    if (d?.kind === "links") await actions.renameNote(d.entry.path, d.name, update);
  }

  function dialogKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      dialog = null;
      void focusRow(focused);
    }
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<nav class="tree" aria-label={t("tree.label")}>
  <form onsubmit={(e) => void submit(e)}>
    <input bind:value={title} placeholder={tf("tree.newNoteIn", { folder: target })} aria-label={tf("tree.newNoteIn", { folder: target })} />
  </form>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <ul
    bind:this={list}
    role="tree"
    aria-label={t("tree.label")}
    onkeydown={keydown}
    oncontextmenu={(e) => e.target === list && openMenu(e, null)}
    ondragover={(e) => e.target === list && dragOver(e, "")}
    ondrop={(e) => e.target === list && void drop(e, "")}
    class:drop={dropOn === ""}
  >
    {#each entries as entry (entry.path)}
      <li role="none" style:padding-left={`calc(${entry.depth} * var(--space-4))`}>
        {#if renaming?.path === entry.path}
          <input
            class="rename"
            bind:value={renaming.value}
            aria-label={entry.is_dir ? t("tree.folderName") : t("tree.noteName")}
            onkeydown={(e) => {
              if (e.key === "Enter") void commitRename();
              else if (e.key === "Escape") { renaming = null; void focusRow(entry.path); }
            }}
            onblur={() => (renaming = null)}
          />
        {:else}
          <button
            class="row"
            class:dir={entry.is_dir}
            class:active={entry.path === selected}
            class:focused={entry.path === focused}
            class:drop={dropOn === entry.path}
            role="treeitem"
            aria-selected={entry.path === selected}
            data-row={entry.path}
            tabindex={entry.path === (focused ?? entries[0]?.path) ? 0 : -1}
            title={entry.is_dir ? entry.name : stem(entry.name)}
            draggable="true"
            onclick={() => {
              focused = entry.path;
              if (!entry.is_dir) onOpen(entry.path);
            }}
            onfocus={() => (focused = entry.path)}
            oncontextmenu={(e) => openMenu(e, entry)}
            ondragstart={(e) => dragStart(e, entry)}
            ondragend={() => (dragging = dropOn = null)}
            ondragover={(e) => entry.is_dir && dragOver(e, entry.path)}
            ondragleave={() => dropOn === entry.path && (dropOn = null)}
            ondrop={(e) => entry.is_dir && void drop(e, entry.path)}
          >
            <Icon icon={entry.is_dir ? Folder : FileText} size="sm" /><span class="name">{entry.is_dir ? entry.name : stem(entry.name)}</span>
          </button>
        {/if}
        {#if creating && entry.is_dir && creating.parent === entry.path}
          <input
            class="create"
            style:margin-left="var(--space-4)"
            bind:value={creating.value}
            placeholder={creating.kind === "note" ? t("tree.noteName") : t("tree.folderName")}
            aria-label={creating.kind === "note" ? t("tree.noteName") : t("tree.folderName")}
            onkeydown={(e) => {
              if (e.key === "Enter") void commitCreate();
              else if (e.key === "Escape") creating = null;
            }}
            onblur={() => (creating = null)}
          />
        {/if}
      </li>
    {/each}
    {#if creating && creating.parent === ""}
      <li role="none">
        <input
          class="create"
          bind:value={creating.value}
          placeholder={t("tree.folderName")}
          aria-label={t("tree.folderName")}
          onkeydown={(e) => {
            if (e.key === "Enter") void commitCreate();
            else if (e.key === "Escape") creating = null;
          }}
          onblur={() => (creating = null)}
        />
      </li>
    {/if}
  </ul>
</nav>

{#if menu}
  {@const entry = menu.entry}
  {@const folder = folderOf(entry)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="tree-menu"
    role="menu"
    tabindex="-1"
    aria-label={tf("tree.menu.label", { name: entry ? entry.name : t("tree.move.root") })}
    style:left={`${menu.x}px`}
    style:top={`${menu.y}px`}
    onkeydown={menuKey}
    onclick={(e) => e.stopPropagation()}
  >
    {#if !entry || entry.is_dir}
      <button role="menuitem" onclick={() => startCreate(folder, "note")}>{t("tree.menu.newNote")}</button>
      <button role="menuitem" onclick={() => startCreate(folder, "folder")}>{t("tree.menu.newFolder")}</button>
      <button role="menuitem" onclick={() => void openTemplates(folder)}>{t("tree.menu.fromTemplate")}</button>
    {/if}
    {#if entry}
      <button role="menuitem" onclick={() => startRename(entry)}>{t("tree.menu.rename")}<kbd>F2</kbd></button>
      <button role="menuitem" onclick={() => ask("move", entry)}>{t("tree.menu.move")}</button>
      <button role="menuitem" class="danger" onclick={() => ask("delete", entry)}>{t("tree.menu.delete")}<kbd>Del</kbd></button>
    {/if}
  </div>
{/if}

{#if dialog}
  <div class="backdrop" aria-hidden="true" onclick={() => (dialog = null)}></div>
  <div class="dialog" bind:this={dialogEl} role="dialog" aria-modal="true" aria-labelledby="tree-dialog-title" tabindex="-1" onkeydown={dialogKey}>
    {#if dialog.kind === "move"}
      <h2 id="tree-dialog-title">{tf("tree.move.title", { name: dialog.entry.is_dir ? dialog.entry.name : stem(dialog.entry.name) })}</h2>
      <div class="choices" role="radiogroup" aria-labelledby="tree-dialog-title">
        {#each moveTargets(entries, dialog.entry.path) as f (f)}
          <label><input type="radio" name="move-to" value={f} bind:group={dialog.choice} />{f || t("tree.move.root")}</label>
        {/each}
      </div>
      <footer>
        <Button variant="quiet" onclick={() => (dialog = null)}>{t("tree.cancel")}</Button>
        <Button variant="primary" onclick={() => void confirm()}>{t("tree.menu.move").replace("…", "")}</Button>
      </footer>
    {:else if dialog.kind === "delete"}
      <h2 id="tree-dialog-title">{t("tree.menu.delete")}</h2>
      <p>{tf(dialog.entry.is_dir ? "tree.delete.folder" : "tree.delete.note", { name: dialog.entry.is_dir ? dialog.entry.name : stem(dialog.entry.name) })}</p>
      <footer>
        <Button variant="quiet" onclick={() => (dialog = null)}>{t("tree.cancel")}</Button>
        <Button variant="danger" onclick={() => void confirm()}>{t("tree.delete.confirm")}</Button>
      </footer>
    {:else if dialog.kind === "links"}
      <h2 id="tree-dialog-title">{t("tree.menu.rename")}</h2>
      <p>{tf("tree.links.ask", { n: dialog.count })}</p>
      <footer>
        <Button variant="quiet" onclick={() => (dialog = null)}>{t("tree.cancel")}</Button>
        <Button onclick={() => void renameWithLinks(false)}>{t("tree.links.keep")}</Button>
        <Button variant="primary" onclick={() => void renameWithLinks(true)}>{t("tree.links.update")}</Button>
      </footer>
    {:else}
      <h2 id="tree-dialog-title">{t("tree.template.title")}</h2>
      {#if dialog.templates.length === 0}
        <p>{t("tree.template.none")}</p>
      {:else}
        <div class="choices" role="radiogroup" aria-labelledby="tree-dialog-title">
          {#each dialog.templates as tpl (tpl)}
            <label><input type="radio" name="template" value={tpl} bind:group={dialog.choice} />{tpl}</label>
          {/each}
        </div>
        <label class="field">{t("tree.noteName")}<input bind:value={dialog.title} onkeydown={(e) => e.key === "Enter" && void confirm()} /></label>
      {/if}
      <footer>
        <Button variant="quiet" onclick={() => (dialog = null)}>{t("tree.cancel")}</Button>
        <Button variant="primary" disabled={!dialog.choice || !dialog.title.trim()} onclick={() => void confirm()}>{t("tree.create")}</Button>
      </footer>
    {/if}
  </div>
{/if}

<style>
  .tree { padding: var(--space-2) 0; }
  form { padding: 0 var(--space-2) var(--space-2); }
  form input { width: 100%; box-sizing: border-box; }
  ul { list-style: none; margin: 0; padding: 0 var(--space-1) var(--space-6); min-height: 4rem; }
  ul.drop { background: var(--color-accent-subtle); }
  .row { display: flex; align-items: center; gap: var(--space-2); min-height: 1.75rem; padding: 0 var(--space-2); box-sizing: border-box; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  button.row {
    width: 100%; background: none; border: 0; text-align: left; cursor: pointer; font: inherit;
    border-radius: var(--radius-md); color: var(--color-text-muted);
    box-shadow: inset 2px 0 0 transparent;
    transition: background-color var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);
  }
  button.row.dir { font-weight: 600; }
  button.row:hover { background: var(--color-surface-hover); color: var(--color-text); }
  button.row.focused { color: var(--color-text); }
  button.row:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  button.row.active { background: var(--color-surface-selected); color: var(--color-text); box-shadow: inset 2px 0 0 var(--color-accent); }
  button.row.drop { background: var(--color-accent-subtle); color: var(--color-text); }
  .rename, .create {
    width: calc(100% - var(--space-2)); box-sizing: border-box; margin: 1px 0; font: inherit; font-size: var(--text-md);
    color: var(--color-text); background: var(--color-surface-raised); border: 1px solid var(--color-accent);
    border-radius: var(--radius-md); padding: 2px var(--space-2);
  }
  .tree-menu {
    position: fixed; z-index: 45; min-width: 12rem; padding: var(--space-1); display: flex; flex-direction: column;
    background: var(--color-surface-raised); border: 1px solid var(--color-border); border-radius: var(--radius-md); box-shadow: var(--shadow-raised);
  }
  .tree-menu button {
    display: flex; justify-content: space-between; gap: var(--space-4); padding: var(--space-1) var(--space-2); border: 0; border-radius: var(--radius-sm);
    background: none; color: var(--color-text); font: inherit; font-size: var(--text-md); text-align: left; cursor: pointer;
  }
  .tree-menu button:hover, .tree-menu button:focus-visible { background: var(--color-accent-subtle); outline: none; }
  .tree-menu .danger { color: var(--color-danger); }
  kbd { font-family: var(--font-ui); font-size: var(--text-xs); color: var(--color-text-muted); }
  .backdrop { position: fixed; inset: 0; background: var(--color-scrim); z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; top: 20vh; left: 50%; transform: translateX(-50%); width: min(26rem, calc(100vw - 2 * var(--space-4)));
    background: var(--color-surface-reading); border: 1px solid var(--color-border); border-radius: var(--radius-lg); box-shadow: var(--shadow-raised);
    padding: var(--space-4); display: flex; flex-direction: column; gap: var(--space-3);
  }
  h2 { margin: 0; font-size: var(--text-lg); font-weight: 600; }
  p { margin: 0; font-size: var(--text-md); }
  .choices { display: flex; flex-direction: column; gap: var(--space-1); max-height: 40vh; overflow-y: auto; }
  .choices label { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-md); cursor: pointer; }
  .choices input { accent-color: var(--color-accent); }
  .field { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); }
  .field input {
    font: inherit; font-size: var(--text-md); color: var(--color-text); background: var(--color-surface-raised);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-md); padding: var(--space-1) var(--space-2);
  }
  footer { display: flex; justify-content: flex-end; gap: var(--space-2); }
</style>
