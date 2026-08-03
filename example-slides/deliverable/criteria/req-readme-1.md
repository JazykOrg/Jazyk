---
requirement: req:readme-1
hash: 909ccede987526ef
---


# Statement

The slide deck shall give an outline of what Jazyk is.

# Quote

> This documentation define a set of slides that give an outline of what Jazyk is.

# Implementing files

- build_deck.py (assembles the deck)
- slide_intro.py (introduces Jazyk by name and site)
- slide_about.py (describes what Jazyk is)
- dist/jazyk-slides.pptx (the built artifact to judge)

# Steps to confirm

1. Build the deck: `python3 build_deck.py` from the deliverable directory.
2. Open `dist/jazyk-slides.pptx` (or extract its slide texts programmatically).
3. Read the deck end to end as a viewer would.
4. Judge whether the slides, taken together, give an outline of what Jazyk is:
   the Introduction slide names Jazyk, and the About slide explains what Jazyk
   is in its own content (not a placeholder).

# Verdict contract

PASS if the deck's slides collectively identify Jazyk and describe what it is,
with real content and no placeholder filler. FAIL otherwise. State reasoning
with the slide texts as evidence.
