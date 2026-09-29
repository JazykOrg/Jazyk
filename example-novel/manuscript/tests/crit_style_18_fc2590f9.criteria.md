# crit_style_18_fc2590f9

Criterion: A change of speaker in dialogue starts a new paragraph.

## Pass conditions

The manuscript must start a new paragraph whenever the speaker changes:

1. When dialogue passes from one character to another, the new speaker's utterance begins a new paragraph.
2. Two different speakers' lines are never joined in the same paragraph.
3. A single speaker's continuous speech may continue within one paragraph, but a turn to a different speaker breaks it.
4. The rule holds across every chapter, not just in isolated passages.

## Verdict contract

This is a positive, judgment-based criterion. It is judged by reading the manuscript chapter files:
- PASS when every change of speaker in dialogue begins a new paragraph, and no paragraph joins two different speakers' lines.
- FAIL when two different speakers' lines appear in the same paragraph, or when a speaker change does not start a new paragraph.

When no chapter files exist yet, the manuscript is not generated and the criterion fails by design: it is the acceptance gate generation must clear.
