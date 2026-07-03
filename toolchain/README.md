# Author toolchain

## Create your world (recommended)

**[Use GitHub template → mapkeeper-world-template](https://github.com/plutork/mapkeeper-world-template/generate)**

One click: new repo with `mapkeeper.toml`, lore/DB folders, and `/user` pre-installed. Open that repo in Cursor — not MAPKEEPER.

Details: [template/README.md](template/README.md).

## Layout in this repo

| Path | Purpose |
|------|---------|
| [template/world/](template/world/) | Canonical scaffold (published to GitHub template repo) |
| [cursor/user.md](cursor/user.md) | Legacy manual install — dogfood only |

## Later (V0+)

- `schemas/` — profile and validation templates
- `cursor/user-*.md` — specialist lenses (Geo, Time, Import, Canon)
- CLI init — optional wrapper around the same scaffold

Maintainer tooling lives outside this public repo.
