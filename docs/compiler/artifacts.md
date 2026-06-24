# Build Artifacts

Compilation and linking produce several artifacts. An artifact is the machine-readable form of the
documentation at a particular phase. Downstream usages read final artifacts, not the input docs.

There are two intermediate artifacts, one final artifact plus a diagnostics store.

## Object artifact

One per documentation file. It is the output of [compilation](./compilation.md). It holds the file's
sections, entities, requirements, and consolidated relationships, plus the entities it expects to
resolve in other files.

This is the translation unit of Jazyk, the equivalent of a `.o` object file.

[See more](./artifacts/object-artifact.md)

## Linked artifact

Whole program. It is the output of the resolve stage of [linking](./linking.md) (steps L1 to L3). It
holds the global entities (the symbol table), the global relationship graph, and a requirement index.

[See more](./artifacts/linked-artifact.md)

## Reviewed artifact

Whole program. It is the output of the validation stage of [linking](./linking.md) (steps L4 to L6).
It extends the linked artifact with synthesized entity definitions, semantic diagnostics, and
coverage.

[See more](./artifacts/reviewed-artifact.md)

## Diagnostics store

Warnings and errors are persisted in a store keyed by a stable id. They survive recompilation and are
consumed by IDEs and CI.

[See more](./artifacts/diagnostics-store.md)

## Reproducibility

Artifacts store the verbatim source, so the original documentation can be reconstructed from them.

[See more](./artifacts/reproducibility.md)
