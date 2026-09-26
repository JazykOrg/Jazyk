"""Requirement req:template-4 (hash 6936c57439477a7d): footer bottom right shows author Matus Faro."""

import sys

from pptx import Presentation
from pptx.enum.text import PP_ALIGN

AUTHOR = "Matus Faro"


def _shape_text(shape):
    if not getattr(shape, "has_text_frame", False):
        return ""
    return "\n".join(
        "".join(run.text for run in paragraph.runs)
        for paragraph in shape.text_frame.paragraphs
    )


def crit_template_2_abf041b5():
    prs = Presentation("dist/jazyk-slides.pptx")
    assert len(prs.slides) > 0, "deck has no slides to inspect for footer author"

    missing = []
    for index, slide in enumerate(prs.slides, start=1):
        candidates = []
        for shape in slide.shapes:
            if AUTHOR not in _shape_text(shape):
                continue
            right_edge = shape.left + shape.width
            bottom_edge = shape.top + shape.height
            in_bottom = shape.top >= prs.slide_height * 0.75
            in_right = right_edge >= prs.slide_width * 0.90
            aligned_right = any(
                paragraph.alignment == PP_ALIGN.RIGHT
                for paragraph in shape.text_frame.paragraphs
            )
            if in_bottom and in_right and aligned_right:
                candidates.append(shape)
        if not candidates:
            missing.append(index)

    assert not missing, (
        f"slides missing {AUTHOR!r} in a right-aligned bottom-right footer area: {missing}"
    )


def _main():
    requested = sys.argv[1] if len(sys.argv) > 1 else "crit_template_2_abf041b5"
    tests = {"crit_template_2_abf041b5": crit_template_2_abf041b5}
    if requested not in tests:
        raise SystemExit(f"unknown test {requested!r}; expected one of {sorted(tests)}")
    tests[requested]()
    print(f"PASS {requested}")


if __name__ == "__main__":
    _main()
