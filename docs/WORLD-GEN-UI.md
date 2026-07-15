# World Build Wizard — UI architecture (v1)

> Maintainer spec — fullscreen build wizard for initial world creation.  
> **Decisions:** D-57 (layout) · D-58 (English product UI) · D-59 (draft + Home layout) · D-77 (tester first-run).

## Home layout (D-59)

Three-card grid — **left Build World equal width to center**:

```text
| Build World (1.2fr) | Your worlds (1.2fr) | Community worlds soon (0.65fr) |
```

| Card | State |
|------|-------|
| **Left — Build World** | Active; name · folder · size · **Start build** |
| **Center — Your worlds** | Project list + blank **Create** |
| **Right — Community worlds** | Disabled placeholder («soon»); no maintainer fixture imports on Home |

**Amends D-35:** left card no longer narrower than center (Build path must stay readable).

Maintainer river dogfood fixtures (`fixtures/worlds/`, `/api/fixture-worlds`) stay for CI — not exposed on Home.

## Tester first-run (D-77)

When `Your worlds` is empty, Home shows one primary onboarding action:

- **Create your first world** (primary CTA) → starts **Build World wizard** directly.
- Defaults: `my-first-world` (collision-safe), `Documents/MAPKEEPER Worlds`, preset **Small**.
- Blank **Create** remains available under **Advanced options** (demoted, not removed).

After **Finish**, the editor shows a small one-shot "what's next" note:

- world is saved,
- reopen from Home with **Open**,
- continue with Inspect / Terrain,
- send feedback if confusing.

## Draft worlds (D-59)

Build-flow worlds persist draft state in **`mapkeeper.toml [build]`** — not in `projects.json` alone.

```toml
[build]
status = "draft"
step = 3
```

| Field | Meaning |
|-------|---------|
| `status` | `draft` while wizard incomplete; `complete` or section absent after Finish |
| `step` | Last wizard step (v1: always 3 — land silhouette) |

### Lifecycle (D-106 track 3 — amends D-59)

```text
Start build → world with [build] draft (build_draft_active in web state)
Wizard | Editor toggle → persist draft on Editor switch; resume layers on Wizard switch
Save Draft / ← Worlds → draft on disk (PUT /api/build) — ← Worlds no longer drops draft
Your worlds → badge Draft + step hint
Open draft → wizard resume at saved step (Home Open still works)
Finish → clear draft session → editor mode (no mode toggle)
Blank Create → no [build] → Open → editor only
```

| API | Role |
|-----|------|
| `POST /api/projects` + `build_wizard: true` | Scaffold with `[build]` draft (Build World only) |
| `PUT /api/build` | `{ status: "draft", step }` or `{ status: "complete" }` on active world |

**Re-entry from editor:** **Wizard | Editor** toggle during an active build draft — no Home-only path required. `build_draft_active` tracks session; `workspace-build` CSS stays until Finish or ← Worlds.

**Deferred:** Home three-card layout changes (non-goal track 3).

## Product intent

MAPKEEPER uses a **unified World Workspace** (D-106) for every open world — hex canvas center, **Wizard | Editor** mode toggle, collapsible left/right panels. The legacy **fullscreen build wizard overlay** (D-57) is replaced by the workspace shell (track 0); wizard step content lives in side panels with **freeze list + generation categories** (track 1).

**UI language (D-58):** all author-facing in-app strings are **English** (Home, wizard, editor, tool-dock). Maintainer chat/docs may stay bilingual.

## Unified workspace shell (D-106 track 0)

| Region | Wizard mode (build flow) | Editor mode |
|--------|--------------------------|-------------|
| Top | ← Worlds · Save Draft · **Editor \| Wizard** · step crumb · status | Top bar (build controls hidden) |
| Left | **Freeze list** (pipeline steps) | **Categories** (Inspect, Terrain, Lakes & rivers) — View→Layers ▾, World→Settings ▾ per ui-shell-redesign Track 1 |
| Center | Hex canvas (always) | Hex canvas |
| Right | Step panels + Generate | **Settings** (per category) + **Objects** list |

**Built (track 3):** D-59 lifecycle amend — draft persist on ← Worlds / Editor switch (`build_draft_active` in `wizard.rs`).

## Mode-first shell (ui-shell-redesign Track 1)

Supersedes the two-button **Wizard | Editor** toggle with a **5-mode top nav**: **Wizard · Generator · Editor · Agent · History**. `WorkspaceMode` (`state.rs`) is the single source of truth; DOM `workspace-mode-*` classes and side panels are derived, never independent booleans.

| Top zone | Content |
|----------|---------|
| **Start** | `← Worlds` (primary nav) · compact **read-only** world label (ellipsis) · Save Draft (build-only) · crumb |
| **Center** | 5-mode nav (`#workspace-mode-nav`, `data-workspace-mode`) |
| **End** | mode note · autosave status · **Layers ▾** (visibility overlays popover) · **Settings ▾** (world info + editable name) |

**Switch pipeline (`wizard.rs`):** `request_workspace_mode` → `evaluate_wizard_entry` / `validate_mode_availability` seams → `leave_current_mode` (Wizard flushes + persists draft) → set SoT → `apply_shell_layout` → `enter_new_mode`. Thin per-mode adapters only over this shared path.

**Mode contracts:**
- **Editor brush is not reset on mode switch.** The saved brush survives leaving Editor; paint/stamp mutations are Editor-gated in the event handlers; pan/zoom are routed by mode (not by forcing `Inspect`).
- **Generator / Agent** are Track 1 **stubs** (visible, read-only, write nothing).
- **History** auto-expands the existing D-107 timeline; side panels are neutral placeholders. **Legacy availability is only isolated** here (single locus in `sync_history_ui`; `build_draft_active` stays out of `WorkspaceMode`) — Track 1 does not yet remove the draft-state dependency.
- **Wizard entry** uses the legacy policy behind `evaluate_wizard_entry` (enterable during an active build draft); a non-draft world shows an informational note until the future `wizard-reset-contract` replaces this policy.
- **Layers ▾** hosts the former View overlays (color mode, grid, elevation labels, peaks + legend/stats); **Settings ▾** hosts world info + the editable `#world-name`; the duplicate `Switch world` button was removed (`← Worlds` is canonical).

**Non-goals (Track 1):** generator toolset, Agent-zone schema, layer ordering, History inheritance/diff, draft/complete lifecycle removal, server/schema changes.

## Editor panels — categories + objects (D-106 track 2)

| Region | Content |
|--------|---------|
| **Left** | **Categories** — migrated from tool-dock rail (Inspect, Terrain, Lakes & rivers). View overlays and World info moved to global `Layers ▾` / `Settings ▾` (ui-shell-redesign Track 1). |
| **Right (top)** | **Settings** — former dock-drawer panes (brushes, generate). Always visible in workspace; no overlay collapse. |
| **Right (bottom)** | **Objects** — lakes + named/legacy rivers on Rivers category; named cells on Inspect. Legacy river **Delete** when catalog writable. |

**Amends D-38/D-39:** tool-dock overlay removed in workspace; panels are in-frame. Escape deselects active brush instead of hiding drawer.

## Wizard panels — tier groups + freeze (D-106 track 1; tier stubs 2026-07)

| Region | Content |
|--------|---------|
| **Left** | **Tier groups** (`#wiz-tier-nav`) — collapsible Setup / Geo / Climate / Water / Soils / Life / Extras / Validation. Built steps **1–6** keep freeze states (pending · active · frozen · stale); click a reached step to jump back (persist draft). Locked steps **7–12** are clickable **informational stubs** (`data-stub="1"`). |
| **Right** | Built step panels (1–6) or **stub panel** (`#wiz-panel-stub`) when viewing a locked step. Generation blocks use **`wiz-gen-category`** sections. **Stale banner** when upstream regen invalidates downstream steps. |

**Tier vs progression:** tiers are **presentation only** — no persisted tier state. Freeze/progression belong to individual steps (`wizard_step`, `wizard_peak_step`, accepted flags). **Viewed stub** (`wizard_viewed_stub` in `AppState`) is separate from progression: opening a stub panel does **not** change current step, freeze/stale state, or persisted draft resume position. Reload / resume always restores the real `wizard_step`.

**Stub panel contract:** title · **Coming later** badge · plain-English promise · “Generation is not available in this version.” No Generate / Accept / Continue / Edit / fake presets.

**Invalidation:** regenerating land/layout after geo accepted (or any step after peak > N) marks downstream steps **stale** and clears accepted flags; successful regen on the stale step advances the marker. `wizard_peak_step` tracks furthest reach so back-nav + regen still invalidates correctly.

**Editor (this track):** three categories (Inspect, Terrain, Lakes & rivers); View→Layers ▾, World→Settings ▾ (ui-shell-redesign Track 1). Wizard shows future pipeline roadmap; Editor shows only available authoring tasks.

**Non-goals (tier stubs track):** steps 7–12 generators; editor category expansion; new API.

## Entry

| Surface | Content |
|---------|---------|
| **Home — left card** | Title **Build World** |
| Card fields | World name · folder · map size (existing presets) |
| Primary action | **Start build** → create world + open wizard |

Replaces the old «Generate Random World» card semantics (blank map shortcut).

## Lifecycle (unified workspace — D-106 track 3)

```text
Home card (setup) → workspace wizard mode → Save Draft (anytime)
Wizard | Editor toggle → draft saved on Editor; layers reloaded on Wizard
← Worlds → auto-save draft → Home
Open (draft world) → resume wizard at saved step
Finish → complete [build] → editor mode (toggle hidden)
```

| Action | When |
|--------|------|
| **Save Draft** | Available while `workspace-build`; writes `[build]` draft via `PUT /api/build`. |
| **Wizard \| Editor** | During build draft only; Editor switch flushes paints/stamps + persists draft. |
| **← Worlds** | Auto-saves draft if build session active; closes world → Home. |
| **Open (draft world)** | Resumes wizard at saved step. |
| **Finish** | Marks build complete; ends draft session; editor panels only. |
| **Re-entry** from editor | **In v1 (track 3):** mode toggle — no separate Home path required. |

While a build draft is active:

- Unified workspace frame (not fullscreen overlay).
- **Wizard \| Editor** toggle visible; Editor exposes categories + settings/objects panels.
- `build_draft_active` in `AppState` — reliable persist even if DOM class timing differs.

## Layout (wizard shell)

```text
Top:    Save Draft · Undo · breadcrumb «Geo › Land silhouette»
Left:   grouped steps (see Groups)
Center: hex map / variant preview
Right:  hint · step controls · warnings (downstream stale)
Bottom: [A][B][C] · Regenerate · Accept · Edit · Continue
```

### Groups (left rail) — built

English tier labels; **collapsed by default** except the tier containing the **progression** step (or the tier of a **viewed stub**). Tier “complete” styling is computed when all built steps in the tier are frozen/stale — not persisted.

| Tier | Built steps | Locked stubs |
|------|-------------|--------------|
| **Setup** | 1 Map size | — |
| **Geo** | 2 Land · 3 Tectonics · 4 Elevation | — |
| **Climate** | 5 Climate | — |
| **Water** | 6 Lakes & rivers | — |
| **Soils** | — | 7 Soil foundation |
| **Life** | — | 8 Biomes |
| **Extras** | — | 9 Resources · 10 Hazards · 11 Points of interest |
| **Validation** | — | 12 Validate world |

**Coast distance** stays an automatic derived layer (no pipeline row). Canonical step numbers **1–6** match `WORLD-GENERATION-PIPELINE.md` / `BUILD_STEP_*` — stubs use **7–12** as roadmap placeholders only.

Legacy D-56 eighteen-step preview in older sections is superseded by this tier map for wizard navigation.

### Right column (v1 pattern)

Per-step panel — **no sliders in v1**:

1. Short **hint** / explanation (plain language).
2. **Style buttons** (2–4 presets, not random seeds).
3. At most **1–2 simple params** (e.g. toggle or enum buttons).
4. **Warnings** when regen would invalidate downstream layers (stub in v1).

Advanced numeric controls (e.g. «coast complexity 0.65») → **Later** advanced panel.

### Bottom bar

- **Variants A / B / C** — parallel generated previews for the current step.
- **Regenerate** — new variants with current style selection.
- **Accept** — commit active variant to the world layer.
- **Edit** — land/water brush on accepted layer (step 3).
- **Continue** — advance when step accepted (v1: triggers Finish flow after land_mask).

## V1 scope (this build slice)

| In scope | Out of scope |
|----------|--------------|
| Unified workspace shell + panels | Climate, rivers, biomes generators (beyond current steps) |
| Home card **Build World** + **Start build** | Home three-card layout redesign |
| Steps 1–6 pipeline + tier nav + locked stubs 7–12 | Steps 7–12 generators / API |
| Save Draft + Finish + mode toggle lifecycle | Sliders / advanced params |
| Locked preview of future groups | Dense cell lists in object panel |

**Backing todo:** `todo/world-pipeline--land-silhouette-v1.md`

## Step 3 — land/ocean silhouette (right panel example)

**Style** (landmass layout):

- One continent
- Archipelago
- Two landmasses
- Island

**Character** (coastline feel):

- Smooth shores
- Jagged shores

Style combinations map to generator presets internally; author never sees raw noise parameters.

## Relation to editor (post-wizard)

After **Finish**:

- Standard editor workspace (categories left, settings/objects right per D-106 track 2).
- Elevation, rivers, profiles work as today.
- Land/water may still be edited via terrain tools until dedicated `land_mask` brush ships in dock (Later).

## Option tracks (Later)

| Slug | Note |
|------|------|
| `world-gen-advanced-params` | Sliders / expert panel per step |
| `world-gen-wizard-mid-pipeline` | Unlock steps 7+ in wizard as generators ship |
