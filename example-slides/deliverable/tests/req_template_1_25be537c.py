"""Requirement req:template-1: all slides use the Microsoft PowerPoint file format."""

import sys
import zipfile

from pptx import Presentation


ARTIFACT = "dist/jazyk-slides.pptx"


def crit_template_3_570446d4():
    """A presenter opening the slide artifact sees it recognized as a Microsoft PowerPoint file."""
    with zipfile.ZipFile(ARTIFACT) as package:
        names = package.namelist()
        assert "[Content_Types].xml" in names, "PowerPoint package content types are missing"
        assert "ppt/presentation.xml" in names, "PowerPoint presentation part is missing"
        content_types = package.read("[Content_Types].xml").decode("utf-8")

    assert "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml" in content_types, (
        "artifact is not identified as a PowerPoint presentation"
    )
    presentation = Presentation(ARTIFACT)
    assert len(presentation.slides) > 0, "PowerPoint artifact opens but contains no slides"


if __name__ == "__main__":
    requested = sys.argv[1] if len(sys.argv) > 1 else "crit_template_3_570446d4"
    if requested != "crit_template_3_570446d4":
        raise SystemExit(f"unknown test: {requested}")
    crit_template_3_570446d4()
    print("PASS crit_template_3_570446d4")
