Status: IDEA (product concept)

Product concept for **mapkeeper** — standalone, no consumer vault assumed.

---

## Conversation mode

Default: **Shape** — questions and tradeoffs first; no file trees / schemas / code unless asked.

| Mode | When |
|------|------|
| **Shape** | product unclear, no V0 |
| **Build** | V0 agreed; editor and contracts ship from this repo |
| **World** | editor testable; author works in a separate world repo |

---

## Pitch

**mapkeeper** is a generic **local** world editor built for the age of AI agents.

**Target author:** writer / game master — not a developer. Pre-configured agents and project layout help them build worlds without reading editor source or using git.

Authors create or import a map, divide it into **addressable cells**, attach **structured canon** to places, **track how canon changes across time**, and expose the **same data** to Cursor agents through commands, schemas, validation, and **queryable profiles**.

**The map is not only a picture. It is the interface to a machine-readable world.**

Under the hood mapkeeper is a **world-state editor**: each hex cell is an addressable container for **partial** world state, split into **layers** (terrain today; elevation, water, regions, routes… later). A cell value can be *unknown* (not decided), *none* (explicitly absent), or a concrete *value* — you fill only what matters now. Brushes, and one day generators, edit these layers; the renderer just projects them. Human-facing place descriptions (**profiles**) stay separate from this machine-readable map state, both anchored by the same cell id.

| Not | Yes |
|-----|-----|
| Azgaar + AI | Generic local world editor |
| Azgaar clone | Map as machine-readable workspace |
| AI runtime inside the product | Agents operate outside; mapkeeper ships **data contracts** + **toolchain templates** |
| One private lore baked in | Private worlds via **adapters** outside core |

**Telemetry:** local-only; no remote collection in core.

---

## Invariants *(stable)*

- Map → machine-readable canon, not decorative PNG.
- Same data for **author** (visual editor) and **agents** (queryable profiles).
- **No AI runtime in product** — agents operate outside; mapkeeper ships data contracts + toolchain templates.
- Core stays **world-agnostic**; adapters connect private worlds **outside** core.
- **Shape first** — no trees/schemas/code until the product moves to Build.

---

## Milestones *(intent only)*

| Horizon | Intent |
|---------|--------|
| **Now** | product idea clear; public kit ready for authors |
| **Next** | V0: local editor + map + profile query; first author friction from real worlds |
| **Later** | canon edit, time slices, adapters, export; user role templates shipped with mapkeeper |

---

## Explicitly deferred (product)

- Repository layout, CSV/JSON schemas, cell_id format, coordinate system
- UI stack, renderer, brushes, temporal model, adapter contract
- Package name, demo world content

---

*2026-07-03 — product concept.*
