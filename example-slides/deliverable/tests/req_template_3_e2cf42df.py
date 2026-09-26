"""Criterion crit:template-1: a slide shows https://jazyk.org with a copyright in the footer's bottom-left area."""

import sys

from pptx import Presentation

SITE = "https://jazyk.org"
TEST_NAME = "crit_template_1_f9621b1f"


def _iter_text_shapes(slide):
    for shape in slide.shapes:
        if shape.has_text_frame:
            yield shape, shape.text_frame.text


def crit_template_1_f9621b1f():
    prs = Presentation("dist/jazyk-slides.pptx")
    mid_x = prs.slide_width // 2
    mid_y = prs.slide_height // 2
    assert len(prs.slides) > 0, "presentation has no slides"

    for idx, slide in enumerate(prs.slides, start=1):
        found = False
        for shape, text in _iter_text_shapes(slide):
            if SITE in text and "©" in text:
                assert shape.left < mid_x, "slide %d: site+copyright is not in the left half" % idx
                assert shape.top > mid_y, "slide %d: site+copyright is not in the bottom half" % idx
                found = True
        assert found, "slide %d has no footer text containing site and copyright" % idx


if __name__ == "__main__":
    requested = sys.argv[1] if len(sys.argv) > 1 else TEST_NAME
    if requested != TEST_NAME:
        raise SystemExit("unknown test %s" % requested)
    crit_template_1_f9621b1f()
    print("PASS %s" % TEST_NAME)
    sys.exit(0)
