"""Criterion test for req:template-6: slides use Comic Sans MS throughout."""

import sys
from pptx import Presentation

EXPECTED_FONT = "Comic Sans MS"
TEST_NAME = "crit_template_6_71315544"


def iter_runs(prs):
    for slide_index, slide in enumerate(prs.slides, start=1):
        for shape_index, shape in enumerate(slide.shapes, start=1):
            if not getattr(shape, "has_text_frame", False):
                continue
            for paragraph_index, paragraph in enumerate(shape.text_frame.paragraphs, start=1):
                for run_index, run in enumerate(paragraph.runs, start=1):
                    if not run.text:
                        continue
                    location = (
                        f"slide {slide_index}, shape {shape_index}, "
                        f"paragraph {paragraph_index}, run {run_index} ({run.text!r})"
                    )
                    yield location, run


def crit_template_6_71315544():
    prs = Presentation("dist/jazyk-slides.pptx")
    checked = 0
    wrong = []
    for location, run in iter_runs(prs):
        checked += 1
        if run.font.name != EXPECTED_FONT:
            wrong.append(f"{location}: font is {run.font.name!r}")
    assert checked > 0, "presentation contains no text runs to check"
    assert not wrong, "text runs not rendered in Comic Sans MS:\n" + "\n".join(wrong)


if __name__ == "__main__":
    selected = sys.argv[1] if len(sys.argv) > 1 else TEST_NAME
    if selected != TEST_NAME:
        raise SystemExit(f"unknown test {selected!r}")
    crit_template_6_71315544()
    print(f"PASS {TEST_NAME}")
