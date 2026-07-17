# map-v2 retirement

Statuses:

- `keep` — still useful for research.
- `review` — not yet distilled.
- `captured` — useful conclusions are documented elsewhere.
- `delete-ready` — replacement is verified and the archive has no research value.
- `rejected` — do not transfer this solution to the new architecture.

Deletion requires all of: review complete, useful conclusions captured in the
new architecture docs or ADR, replacement coverage implemented and verified,
and no remaining research value.

| Subsystem | Status | Retirement note |
|---|---|---|
| Map rendering | review | Compare interaction/performance lessons; reject renderer contracts |
| Viewport and interactions | keep | Input routing may inform a future neutral viewport |
| Dense layers | review | Capture storage tradeoffs only after new requirements exist |
| World storage and transactions | keep | Preserve reliability patterns, not the map artifact graph |
| Editor brushes | rejected | Domain and coordinate assumptions must not seed new tools |
| Selection and inspector | review | Revisit only after future selectable subjects are defined |
| Generator and Wizard | review | Capture test discipline; do not preserve pipeline order |
| Hydrology | keep | Graph invariants may remain useful research material |
| Rivers and lakes | review | Domain behavior is reference-only |
| History | review | CoW idea may be useful; map-domain revision contract is rejected |
| Schemas and API contracts | rejected | No active or future compatibility promise |
| Tests and fixtures | keep | Seeded and failure-class guard patterns are useful examples |
| World scaffold | rejected | New worlds use identity-only config during the reset |
| Architecture docs | keep | Historical rationale and failure record |
| Agent helpers | review | Extract only general author/testing lessons |
