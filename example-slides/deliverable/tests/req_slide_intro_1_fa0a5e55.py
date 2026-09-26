"""Criterion test for req:slide-intro-1: the Introduction slide shows headline title `Jazyk`."""

import sys

from pptx import Presentation

TEST_NAME = "crit_slide_intro_1_40941ccb"
PPTX = "dist/jazyk-slides.pptx"


def crit_slide_intro_1_40941ccb():
    prs = Presentation(PPTX)
    intro = list(prs.slides)[0]
    headline = None
    for shape in intro.shapes:
        if shape.has_text_frame and shape.text_frame.text.strip() == "Jazyk":
            headline = shape
            break
    assert headline is not None, "Introduction slide has no `Jazyk` headline"
    run = headline.text_frame.paragraphs[0].runs[0]
    size = run.font.size
    assert size is not None and size.pt >= 40, (
        "the `Jazyk` title is not headline-sized: %r" % size
    )


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] != TEST_NAME:
        raise SystemExit("unknown test name: %s" % sys.argv[1])
    crit_slide_intro_1_40941ccb()
    print("PASS %s" % TEST_NAME)
