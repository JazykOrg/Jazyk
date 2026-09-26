"""Criterion test for req:slide-about-1: the About slide explains Jazyk in two sentences."""

import re
import sys

from pptx import Presentation

TEST_NAME = "crit_slide_about_1_41f9882e"
PPTX = "dist/jazyk-slides.pptx"


def _shape_texts(slide):
    texts = []
    for shape in slide.shapes:
        if getattr(shape, "has_text_frame", False):
            text = shape.text_frame.text.strip()
            if text:
                texts.append(text)
    return texts


def _sentences(text):
    return [s.strip() for s in re.split(r"(?<=[.!?])\s+", text.strip()) if s.strip()]


def _is_footer(text):
    return "jazyk.org" in text or "© Jazyk" in text or "Matus Faro" in text


def crit_slide_about_1_41f9882e():
    prs = Presentation(PPTX)
    about_slides = []
    for slide in prs.slides:
        texts = _shape_texts(slide)
        if any(text == "About Jazyk" for text in texts):
            about_slides.append(texts)

    assert about_slides, "no slide titled 'About Jazyk' found"
    texts = about_slides[0]
    body_texts = [
        text for text in texts
        if text != "About Jazyk" and text != "jazyk" and not _is_footer(text) and text.strip()
    ]
    body = "\n".join(body_texts)

    sentences = _sentences(body)
    assert len(sentences) == 2, (
        "About slide should explain Jazyk in two sentences; got %d sentences in %r"
        % (len(sentences), body)
    )
    lowered = body.lower()
    assert "jazyk" in lowered and "natural language compiler" in lowered, (
        "About slide does not say what Jazyk is; body text: %r" % body
    )
    assert "plain prose documentation" in lowered and "semantic graph" in lowered, (
        "About slide lacks explanatory detail about what Jazyk is about; body text: %r" % body
    )


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] != TEST_NAME:
        raise SystemExit("unknown test name: %s" % sys.argv[1])
    crit_slide_about_1_41f9882e()
    print("PASS %s" % TEST_NAME)
