"""The slide deck shall be delivered in the Microsoft PowerPoint file format."""

import sys
import zipfile

from pptx import Presentation


def req_template_2_ea3dae53():
    path = "dist/jazyk-slides.pptx"
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        assert "ppt/presentation.xml" in names, "not an OOXML presentation package"
        content_types = z.read("[Content_Types].xml").decode("utf-8")
    assert "presentationml" in content_types, "package content types are not PowerPoint"
    # And it must open as a presentation.
    Presentation(path)


if __name__ == "__main__":
    req_template_2_ea3dae53()
    print("PASS req:template-2")
    sys.exit(0)
