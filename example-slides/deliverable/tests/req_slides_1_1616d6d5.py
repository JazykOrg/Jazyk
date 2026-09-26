"""Criterion test for req:slides-1 / crit:slides-1.

The Slides deck is composed of the Introduction slide.
"""

import sys

from pptx import Presentation


TEST_NAME = "crit_slides_1_fe485363"


def crit_slides_1_fe485363():
    prs = Presentation("dist/jazyk-slides.pptx")
    slide_texts = []
    for slide in prs.slides:
        texts = [shape.text_frame.text for shape in slide.shapes if shape.has_text_frame]
        slide_texts.append(texts)
        if any(text.strip() == "Jazyk" for text in texts):
            return
    raise AssertionError(
        "deck does not contain the Introduction slide identified by its `Jazyk` headline; "
        f"slide texts: {slide_texts!r}"
    )


if __name__ == "__main__":
    selected = sys.argv[1] if len(sys.argv) > 1 else TEST_NAME
    if selected != TEST_NAME:
        raise SystemExit(f"unknown test {selected!r}; expected {TEST_NAME}")
    crit_slides_1_fe485363()
    print(f"PASS {TEST_NAME}")
