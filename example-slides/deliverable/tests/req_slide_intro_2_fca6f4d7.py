"""Criterion test for req:slide-intro-2 / crit:slide-intro-2.
Statement hash: e2b74e015d49e432
"""

import sys

from pptx import Presentation

SITE = "https://jazyk.org"


def crit_slide_intro_2_85c5cbe6():
    prs = Presentation("dist/jazyk-slides.pptx")
    intro = list(prs.slides)[0]
    found = False
    for shape in intro.shapes:
        if not shape.has_text_frame:
            continue
        for para in shape.text_frame.paragraphs:
            for run in para.runs:
                if run.hyperlink.address == SITE and SITE in run.text:
                    found = True
    assert found, "Introduction slide has no visible hyperlink to %s" % SITE


TESTS = {
    "crit_slide_intro_2_85c5cbe6": crit_slide_intro_2_85c5cbe6,
}


if __name__ == "__main__":
    name = sys.argv[1] if len(sys.argv) > 1 else "crit_slide_intro_2_85c5cbe6"
    if name not in TESTS:
        print(f"Unknown test {name}", file=sys.stderr)
        sys.exit(2)
    TESTS[name]()
    print(f"PASS {name}")
    sys.exit(0)
