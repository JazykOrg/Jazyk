---
requirement: req:style-19
criterion: crit:style-11
statementHash: 6117d9016621ac63
kind: llm
---

# crit_style_11_80fc520f

Criterion: In Ines's dialogue, the shop is called `the shop` or `the Lantern Room`, and the phrase `the bookshop` does not appear as her wording.

## Pass conditions

The manuscript must keep Ines's own wording for the shop consistent with her voice:

1. In every line of spoken dialogue spoken by Ines, when she refers to the shop she calls it `the shop` or `the Lantern Room`.
2. The phrase `the bookshop` must not appear as Ines's wording: she never says or writes `the bookshop` in her dialogue.
3. The restriction binds Ines's dialogue only: other characters may use `the bookshop`, and narration or description that uses `the bookshop` is not a violation; the phrase must not be Ines's own wording.

## Verdict contract

This is a negative, judgment-based criterion. It is judged by reading the manuscript chapter files in reading order and inspecting Ines's spoken dialogue:
- PASS when, in every line of Ines's dialogue, the shop is called `the shop` or `the Lantern Room`, and the phrase `the bookshop` never appears as her wording.
- FAIL when any line of Ines's dialogue uses the phrase `the bookshop`.

When no chapter files exist yet, the manuscript is not generated and the criterion fails by design: it is the acceptance gate generation must clear.
