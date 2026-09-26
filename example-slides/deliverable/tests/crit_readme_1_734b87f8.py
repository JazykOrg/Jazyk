"""Criterion test for req:readme-1: the deck introduces Jazyk and describes what it is."""

import sys

from pptx import Presentation


def crit_readme_1_734b87f8():
    prs = Presentation("dist/jazyk-slides.pptx")
    slide_texts = []
    for slide in prs.slides:
        texts = []
        for shape in slide.shapes:
            if getattr(shape, "has_text_frame", False):
                text = shape.text_frame.text.strip()
                if text:
                    texts.append(text)
        slide_texts.append("\n".join(texts))

    all_text = "\n".join(slide_texts)
    assert any("Jazyk" in text for text in slide_texts), (
        "deck does not introduce Jazyk by name; slide texts: %r" % slide_texts
    )
    assert "natural language compiler" in all_text, (
        "deck does not describe what Jazyk is as a natural language compiler; slide texts: %r"
        % slide_texts
    )
    assert "plain prose documentation" in all_text and "semantic graph" in all_text, (
        "deck lacks substantive explanatory outline of Jazyk; slide texts: %r" % slide_texts
    )


if __name__ == "__main__":
    crit_readme_1_734b87f8()
    print("PASS crit:readme-1")
    sys.exit(0)
