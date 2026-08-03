"""Build the Jazyk slide deck: a PowerPoint outline of what Jazyk is."""

import os

from pptx import Presentation
from pptx.util import Inches

import slide_about
import slide_intro

OUT_DIR = "dist"
OUT_PPTX = os.path.join(OUT_DIR, "jazyk-slides.pptx")


def build():
    """Assemble the deck in template order: Introduction first, About second."""
    prs = Presentation()
    prs.slide_width = Inches(13.333)
    prs.slide_height = Inches(7.5)

    slide_intro.build(prs)

    slide_about.build(prs)

    os.makedirs(OUT_DIR, exist_ok=True)
    prs.save(OUT_PPTX)
    return OUT_PPTX


if __name__ == "__main__":
    print(build())
