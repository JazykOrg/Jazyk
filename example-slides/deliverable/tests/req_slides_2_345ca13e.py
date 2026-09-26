"""Criterion test for req:slides-2 / crit:slides-2.

The Slides deck is composed of the About slide.
"""

import sys

from pptx import Presentation


TEST_NAME = "crit_slides_2_69940e48"


def crit_slides_2_69940e48():
    prs = Presentation("dist/jazyk-slides.pptx")
    slide_texts = []
    for slide in prs.slides:
        texts = [shape.text_frame.text for shape in slide.shapes if shape.has_text_frame]
        slide_texts.append(texts)
        if any("About" in text for text in texts):
            return
    raise AssertionError(
        "deck does not contain an About slide identified by visible text; "
        f"slide texts: {slide_texts!r}"
    )


if __name__ == "__main__":
    selected = sys.argv[1] if len(sys.argv) > 1 else TEST_NAME
    if selected != TEST_NAME:
        raise SystemExit(f"unknown test {selected!r}; expected {TEST_NAME}")
    crit_slides_2_69940e48()
    print(f"PASS {TEST_NAME}")
