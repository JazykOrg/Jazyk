"""Criterion test for req:template-2: All slides have a footer.

Statement hash: b52bad4868f168c8
"""

import sys

from pptx import Presentation
from pptx.util import Inches

TEST_NAME = "crit_template_4_700dfd8e"
PPTX = "dist/jazyk-slides.pptx"


def _bottom_texts(slide):
    """Return visible text from shapes positioned in the footer area."""
    footer_top = Inches(6.5)
    texts = []
    for shape in slide.shapes:
        if not shape.has_text_frame:
            continue
        text = shape.text_frame.text.strip()
        if text and shape.top >= footer_top:
            texts.append(text)
    return texts


def crit_template_4_700dfd8e():
    prs = Presentation(PPTX)
    slides = list(prs.slides)
    assert len(slides) >= 2, "deck does not contain both Introduction and About slides"

    for index, label in ((0, "Introduction"), (1, "About")):
        texts = _bottom_texts(slides[index])
        combined = "\n".join(texts)
        assert texts, "%s slide has no visible footer text at the bottom" % label
        assert "https://jazyk.org" in combined, "%s slide footer lacks the site link" % label
        assert "© Jazyk" in combined, "%s slide footer lacks the copyright text" % label
        assert "Matus Faro" in combined, "%s slide footer lacks the author text" % label


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] != TEST_NAME:
        raise SystemExit("unknown test name: %s" % sys.argv[1])
    crit_template_4_700dfd8e()
    print("PASS %s" % TEST_NAME)
