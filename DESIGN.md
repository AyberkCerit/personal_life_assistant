# PLA design

How PLA looks and why. Read this before drawing a new screen; the tokens live in
`apps/desktop/src/theme/tokens.css` and the shared components in `apps/desktop/src/lib/ui/`.

## Principles

1. **Calm workspace, one signature.** Neutral "night blue" surfaces; mint-teal is the only brand colour.
2. **The note comes first.** The reading area is the lightest surface in the dark theme; panels sit one step darker.
3. **AI is always marked, and only AI.** The `sparkles` icon with the `--color-ai*` colours means "the model made or asks about this". Nothing else uses it. An AI-made task keeps its badge after the user ticks, snoozes or edits it (`aiMark` in `lib/tasks.ts`).
4. **Readable, not loud.** Dark text contrast ≈ 11 : 1, never pure white on pure black; every text pair meets WCAG AA in both themes.
5. **Quiet motion.** Short fades and slides that explain a change; none when the OS asks for reduced motion.

## Tokens

Use tokens, never raw colours (a test fails on hex, `rgb(` or `hsl(` outside `src/theme/`).

| Token | Use |
|---|---|
| `--color-surface-reading` | editor / centre pane, page background |
| `--color-surface-panel` | sidebar, task panel, status bar |
| `--color-surface-raised` | toasts, menus, cards |
| `--color-surface-hover` / `-selected` | row hover / selected row |
| `--color-border` / `-strong` | dividers, inputs / checkbox outline, focused input |
| `--color-text` / `-muted` | body text / secondary text |
| `--color-accent`, `--color-on-accent`, `--color-accent-subtle` | primary actions, selection bar, focus ring, "ready" / text on accent / tints and highlights |
| `--color-link`, `--color-link-underline` | wikilinks and links |
| `--color-ai`, `--color-ai-subtle` | AI badge and AI moments only |
| `--color-warning(-subtle)` | paused, soft warnings |
| `--color-danger(-subtle)` | errors, overdue, destructive actions |
| `--color-banner(-border)` | reminder banner |
| `--color-selection`, `--color-active-line` | editor selection / see-through tint of the cursor line (must stay translucent, or it hides the selection) |

There is no separate success green: the accent means "ready / done".

Contrast (both themes, enforced by `src/theme/contrast.test.ts`):
- text on its surfaces ≥ 4.5 : 1 — add every new text/surface pair to `PAIRS`;
- what identifies a control (checkbox and input outlines in `--color-border-strong`, the accent focus ring) ≥ 3 : 1 — `NON_TEXT`;
- placeholders use `--color-text-muted`.

## Type, space, shape, motion

- Font: `--font-ui` (Segoe UI) everywhere, also in the editor; `--font-mono` (Cascadia Mono) only for code.
- Sizes (rem): `--text-xs` 11, `-sm` 12, `-md` 13 (UI default), `-lg` 15 (editor body), `-xl` 17, `-2xl` 20.
- Space: `--space-1` 4 px … `--space-8` 32 px (4 px steps).
- Radius: `--radius-sm` 4 (checkbox, code), `-md` 6 (buttons, inputs), `-lg` 8 (cards, banners), `-full` (badges, dots).
- Shadow: `--shadow-raised` only on raised surfaces.
- Motion: CSS uses `--duration-fast` (120 ms) / `--duration-base` (200 ms) with `--ease-standard`; Svelte transitions use `motion(FAST | BASE)` from `lib/ui/motion.ts`.

## Icons

Lucide only, imported one icon at a time (`import Bell from "@lucide/svelte/icons/bell"`; the package root pulls in ~8 000 files and stalls the tests), drawn through `<Icon icon={…} size="sm|md|lg">` (14/16/20 px, stroke 1.75). Icon-only buttons use `<IconButton label="…">`; the label is required. No emoji as icons.

## Components (`src/lib/ui/`)

| Component | When |
|---|---|
| `Button` | any action with text; `primary` once per area, `secondary` default, `quiet` for minor/cancel, `danger` only to confirm a destructive action |
| `IconButton` | compact actions in rows and banners (delete, close) |
| `Badge` | `ai` for AI items, `warning`/`danger`/`neutral` for small states and counts |
| `Checkbox` | every checkbox (keeps the native input for keyboard and screen readers) |
| `Banner` | messages above the editor: `reminder`, `warning`, `danger` |
| `EmptyState` | any empty list or pane: icon, title, optional hint |
| `Logo` | the P monogram |

## Don't

- Hard-code colours, sizes in `px` for text, or new shadows on flat surfaces.
- Use emoji or other icon sets.
- Use `sparkles` / AI colours for anything the user made.
- Add an animation without the motion tokens (it must stop under reduced motion).
- Add a user-visible string without both `tr` and `en` in `i18n.ts`.

## App icon

Sources in `apps/desktop/src-tauri/icons/source/`; rebuild with `build_icons.py` (see its docstring).
