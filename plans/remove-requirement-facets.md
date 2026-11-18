# Remove requirement classifications

Requirements retain statement, entities, edges, transition and provenance. Remove
requirement classifications, their write gate, placement checks and change record.
Concrete bounds and test shapes remain in criteria. Diagram analysis uses edges,
transitions, containment and criteria. No replacement classification is added.

## Compatibility

Store version stays 7. Ignore obsolete requirement fields when loading accepted
stores; omit them on the next write. Keep criterion kind and measure. Preserve
IDs, provenance, authored diagrams and the one-previous-version store policy.

## Implementation and validation

- Remove model and mutation fields, tools and prompts, checks and change records.
- Keep curate-view query curation. Use transition conditions for sequence branches.
- Remove the isolated timing fallback and read criterion bounds.
- Update benchmark cases and replay fixtures. Preserve recorded run artifacts.
- Test plain requirement creation, old-field round trips, transition rendering,
  removed diagnostics and the updated fixtures.
- Run cargo tests, formatting, Clippy, WASM and a release build. Merge with
  `--no-ff`; coordinate the combined deployment after verified integration.
