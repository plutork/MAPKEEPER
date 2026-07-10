# Product skills (MAPKEEPER)

Build-time depth for V0 — these skills auto-load by file path.

| Skill | When | Complements |
|-------|------|-------------|
| [mapkeeper-cell-schema](mapkeeper-cell-schema/SKILL.md) | `schemas/**` | D-12 profiles + `/real` after Shape |
| [mapkeeper-hex-ui](mapkeeper-hex-ui/SKILL.md) | `src/**` web UI | D-12 hex editor; adapted from anthropics frontend-design |
| [mapkeeper-web-tests](mapkeeper-web-tests/SKILL.md) | `tests/**`, `e2e/**` | V0 dogfood; adapted from anthropics webapp-testing |
| [mapkeeper-world-template](mapkeeper-world-template/SKILL.md) | `toolchain/template/world/**` | D-08/D-10 template sync |

**Gate:** product skills do not bypass approved decision/todo build flow.

**Upstream:** UI/test patterns adapted from [anthropics/skills](https://github.com/anthropics/skills) (see repo LICENSE).
