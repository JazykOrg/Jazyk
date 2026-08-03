---
requirement: req:template-1
hash: 27b3a0484bfead87
---

# Statement

The slide deck shall follow the template conventions for structure.

# Quote

> This defines how all of the [slides](./slides.md) are structured and need to follow the conventions.

# Implementing files

- theme.py (the template constants: colors, font)
- build_deck.py (applies the template order and builds the deck)
- footer.py (the footer convention applied to every slide)
- dist/jazyk-slides.pptx (the built artifact to judge)

# Steps to confirm

1. Build the deck: `python3 build_deck.py` from the deliverable directory.
2. Inspect `dist/jazyk-slides.pptx`.
3. Check each template convention from docs/template.md against the deck:
   PowerPoint format, primary color #248555, complementary color #8e4367,
   Comis Sans font, and a footer on every slide (site with copyright bottom
   left, author Matus Faro bottom right).
4. Judge whether all slides consistently follow these conventions, i.e. no
   slide deviates from the shared structure.

# Verdict contract

PASS if every slide follows the template conventions above. FAIL if any slide
deviates. State which conventions were checked and the evidence per slide.
