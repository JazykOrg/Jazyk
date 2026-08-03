---
requirement: req:slide-about-1
hash: f21bd769
---

# Statement

The About slide shall describe what Jazyk is about in a couple of sentences.

# Quote

> This slide defines what Jazyk is about in a couple of sentences.

# Implementing files

- slide_about.py (the About slide content)
- dist/jazyk-slides.pptx (the built artifact to judge)

# Steps to confirm

1. Build the deck: `python3 build_deck.py` from the deliverable directory.
2. Extract the text of the second slide of `dist/jazyk-slides.pptx`.
3. Check the body text describes what Jazyk is about: real descriptive
   content, not placeholder filler.
4. Count the sentences of the description: "a couple" means the description
   is short, about two sentences, not one fragment and not a page of text.

# Verdict contract

PASS if the About slide carries a real description of Jazyk that is roughly
two sentences long. FAIL if the description is missing, placeholder, a single
fragment, or far longer than a couple of sentences. State the slide text and
the sentence count as evidence.
