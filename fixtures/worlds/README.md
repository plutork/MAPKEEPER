# River dogfood fixture worlds

Curated **Small** (14×8, 112 cells) elevation worlds for manual and auto river
testing. Maintainer/CI assets — not author-facing generator UX (roadmap 7.8).

## Worlds

| Slug | Intent |
|------|--------|
| `coastal-slope` | Ocean west, land rises east — drain to sea |
| `mountain-ridge` | Central ridge, ocean both sides — two drainages |
| `enclosed-basin` | Interior low bowl + hill ring — depression-fill Later |
| `gentle-plain` | Mild north→south slope — subtle routing |
| `dual-watershed` | Two peaks, central valley — opposite coasts |

Each folder is a minimal openable world: `mapkeeper.toml` + `map/manifest.json` +
`map/layers/elevation.json` (dense v2, all cells painted).

## Regenerate

```powershell
python fixtures/worlds/generate_fixture_worlds.py
```

## Open in editor

From repo root (dev):

```powershell
cargo run -p mapkeeper -- server --world "fixtures/worlds/coastal-slope"
```

Or copy a folder to `Documents/MAPKEEPER Worlds/` and open via Home.

**Home UI:** when the server runs from a repo checkout, the right card
**River test maps** lists all five presets — first open copies into
`Documents/MAPKEEPER Worlds/fixture-<slug>` and jumps into the editor.

## Validation

`fixtures/validate_schema.py` checks every world's manifest and elevation layer.
