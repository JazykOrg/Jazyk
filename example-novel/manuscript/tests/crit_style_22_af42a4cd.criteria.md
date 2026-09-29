---
requirement: req:style-4
criterion: crit:style-22
statementHash: 2e97f996aabcecbc
kind: llm
---

# crit_style_22_af42a4cd

Criterion: Each adjacent pair of chapters changes point of view between Ines and Theo.

## Pass conditions

The manuscript must alternate point of view chapter by chapter between Ines and Theo:

1. Reading the chapter files in reading order, each chapter is narrated from the point of view of exactly one of the two characters, Ines or Theo.
2. For every adjacent pair of chapters, the point of view of the second chapter is the other character: an Ines chapter is followed by a Theo chapter, and a Theo chapter is followed by an Ines chapter.
3. No two adjacent chapters share the same point of view; the alternation holds across the whole manuscript, not just within a section.

## Verdict contract

This is a positive, judgment-based criterion. It is judged by reading the manuscript chapter files in reading order and identifying the point of view of each chapter:

- PASS when every chapter is narrated from Ines's or Theo's point of view and each adjacent pair of chapters switches between the two.
- FAIL when any adjacent pair of chapters is narrated from the same point of view, or when a chapter is narrated from a point of view other than Ines's or Theo's.

When no chapter files exist yet, the manuscript is not generated and the criterion fails by design: it is the acceptance gate generation must clear.
