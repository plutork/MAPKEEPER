# Roadmap решений (preliminary)

> **Статус:** черновик из [STARTER_PACK.md](../STARTER_PACK.md). Не ADR — очередь тем для Shape → Build.  
> **Правило:** пункт становится решением только после явного «да» автора (см. MAPKEEPER-OS `decisions.md`).

**Фаза сейчас:** Shape (нет согласованного V0).

---

## Легенда

| Маркер | Значение |
|--------|----------|
| ✅ | Решено |
| 🟡 | Направление есть, детали не зафиксированы |
| ⬜ | Открыто — нужен Shape |
| 🔒 | Инвариант (не пересматриваем без сильной причины) |
| ⏸ | Явно отложено (Later) |

---

## Горизонты (из STARTER_PACK)

| Horizon | Intent | Ключевые решения |
|---------|--------|------------------|
| **Now** | идея ясна; workspace + agent OS | ✅ repo split, ✅ maintainer OS |
| **Next (V0)** | editor + map + profile query; первый User friction | ⬜ всё в блоке V0 ниже |
| **Later** | canon edit, time slices, adapters, export; user role templates | ⏸ блоки 4–6 |

---

## 🔒 Инварианты (не в очереди решений)

- Карта → machine-readable canon, не декоративный PNG
- Одни данные для автора (editor) и агентов (profiles)
- **No AI runtime** в продукте — агенты снаружи
- Core **world-agnostic**; частные миры через **adapters** вне core
- **Telemetry:** local-only
- **Shape first** до перехода в Build

---

## ✅ Уже решено

| # | Решение | Где |
|---|---------|-----|
| D-01 | Public [MAPKEEPER](https://github.com/plutork/MAPKEEPER) + private [MAPKEEPER-OS](https://github.com/plutork/MAPKEEPER-OS); multi-root workspace | OS decisions |
| D-02 | Maintainer memory не в public git | D-01 |
| D-03 | Агент коммитит и пушит в рамках проекта | OS decisions |
| D-04 | `/product`, `/reflect` в MAPKEEPER-OS; user template в `toolchain/cursor/` | layout |

---

## Блок 1 — Переход Shape → Build

*Без этого остальные решения не финализируем.*

| # | Решение | Статус | Tradeoffs / вопросы |
|---|---------|--------|---------------------|
| 1.1 | **V0 scope** — что обязательно в первой версии | ⬜ | Editor only? Map + cells? Profile query CLI? Validation? |
| 1.2 | **Критерий «V0 agreed»** — когда выходим из Shape | ⬜ | Checklist vs одно предложение scope |
| 1.3 | **Repository layout** (product repo) | ⬜ | monorepo vs `src/` + `schemas/` + `toolchain/` |
| 1.4 | **Package name** | ⬜ | npm/cargo/pyPI имя, CLI binary name |
| 1.5 | **License** | ⬜ | OSS vs source-available; влияет на adapters |

**Зависимости:** 1.1 блокирует 2.x, 3.x, 5.x.

---

## Блок 2 — Пространство и адресация (Geo)

*Ядро «addressable cells».*

| # | Решение | Статус | Tradeoffs / вопросы |
|---|---------|--------|---------------------|
| 2.1 | **cell_id format** | ⬜ | human-readable vs opaque; иерархия (region/hex) |
| 2.2 | **Coordinate system** | ⬜ | pixel, geo lat/lon, grid index, multi-layer |
| 2.3 | **Map import для V0** | ⬜ | PNG + manual grid vs SVG vs Azgaar export adapter (Later?) |
| 2.4 | **Spatial queries** в profile | ⬜ | by id only vs neighbors vs bbox |
| 2.5 | **Subdivision** — как делить карту на cells | ⬜ | hex, square, freeform polygon, paint brush semantics |

**Зависимости:** 2.1–2.2 → schemas (3.x), editor UX (4.x).

---

## Блок 3 — Data contract (schemas & profiles)

| # | Решение | Статус | Tradeoffs / вопросы |
|---|---------|--------|---------------------|
| 3.1 | **Формат canon** (CSV / JSON / both) | ⬜ | author editability vs validation |
| 3.2 | **Cell profile schema** — минимальные поля V0 | ⬜ | name, tags, links, lore blob? |
| 3.3 | **Queryable profiles** — как агент читает | ⬜ | files on disk vs CLI `profile query` vs MCP later |
| 3.4 | **Validation rules** — strict vs warn | ⬜ | блокирует save или только lint |
| 3.5 | **World project layout** (author repo) | ⬜ | один world folder vs mapkeeper project file |

**Зависимости:** 1.1 V0 scope; 3.3 связан с 5.2 (world ↔ contracts wiring).

---

## Блок 4 — Editor & UX

| # | Решение | Статус | Tradeoffs / вопросы |
|---|---------|--------|---------------------|
| 4.1 | **UI stack** | ⬜ | web (local) vs desktop (Tauri/Electron) vs hybrid |
| 4.2 | **Renderer** — как рисуем карту | ⬜ | canvas, WebGL, static image overlay |
| 4.3 | **Brushes / tools V0** | ⬜ | select cell, paint id, edit canon panel — минимум? |
| 4.4 | **Canon edit в V0** | 🟡 | STARTER_PACK: canon edit в **Later** — подтвердить |
| 4.5 | **Demo / onboarding** | ⬜ | empty world vs bundled sample (sample **не** в core lore) |

**Зависимости:** 4.1 влияет на всё; 4.4 — ключевой scope tradeoff для V0.

---

## Блок 5 — Agent OS & workspaces

| # | Решение | Статус | Tradeoffs / вопросы |
|---|---------|--------|---------------------|
| 5.1 | **Friction log:** copy vs shared | ⬜ | ручное копирование USER → product journal до V0 (STARTER_PACK TBD) |
| 5.2 | **World repo ↔ data contracts** wiring | ⬜ | path env, relative export, package dependency |
| 5.3 | **World workspace** — когда открывать | 🟡 | «editor testable» — критерии |
| 5.4 | **User lenses** (Geo, Time, Import, Canon) | ⏸ | после повторяющегося friction |
| 5.5 | **Agent OS level 1–3** (rules, stop-hook) | ⏸ | ladder из STARTER_PACK B5 |
| 5.6 | **`npx mapkeeper-init`** для user kit | ⏸ | после V0 |

---

## Блок 6 — Time, adapters, export (Later)

| # | Решение | Статус | Tradeoffs / вопросы |
|---|---------|--------|---------------------|
| 6.1 | **Temporal model** | ⏸ | snapshots vs events vs layers |
| 6.2 | **Time slices** — «what was true when» | ⏸ | query API shape |
| 6.3 | **Adapter contract** | ⏸ | import/export boundary; plugin vs script |
| 6.4 | **Export formats** | ⏸ | for agents, for backup, for publish |
| 6.5 | **User role templates** shipped with product | ⏸ | copy of dogfooded `/user` lenses |

---

## Рекомендуемый порядок Shape-сессий

```
1. V0 scope (1.1) + canon in/out of V0 (4.4)
        ↓
2. cell_id + coordinates (2.1, 2.2) — минимум для «addressable»
        ↓
3. profile schema V0 (3.2) + query path (3.3)
        ↓
4. repo layout + license (1.3, 1.5)
        ↓
5. UI stack (4.1) — после контракта данных, не до
        ↓
6. world workspace criteria (5.3) + friction wiring (5.1)
```

---

## Открытые product tradeoffs (из pitch)

| Вопрос | Зачем сейчас |
|--------|--------------|
| V0 = «read-only canon + map» или сразу edit? | определяет 4.4 и срок world dogfood |
| Cells на import или только manual? | определяет 2.3 и scope import adapter |
| Profile query — file-first или CLI-first? | определяет 3.3 и toolchain V0 |
| Desktop vs local web? | 4.1; влияет на distrib и renderer |

---

## Связь с decisions

Принятые решения дублируются в **MAPKEEPER-OS** `.cursor/agents/product/memory/decisions.md`.  
Этот файл — **очередь и карта**; при «да» пункт переносится в decisions с датой и перечёркивается здесь (или меняет статус на ✅).

---

*2026-07-03 — derived from STARTER_PACK; update after Shape milestones.*
