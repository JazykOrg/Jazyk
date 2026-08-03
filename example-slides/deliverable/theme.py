"""Shared look-and-feel constants for the Jazyk slide deck template."""

from pptx.dml.color import RGBColor
from pptx.util import Pt

PRIMARY = RGBColor.from_string("248555")

COMPLEMENTARY = RGBColor.from_string("8E4367")

FONT = "Comis Sans"

BODY_SIZE = Pt(20)
FOOTER_SIZE = Pt(12)
TITLE_SIZE = Pt(54)


def style_run(run, size=None, color=None, bold=False):
    """Apply the template font to a run, with optional size/color/bold."""
    run.font.name = FONT
    if size is not None:
        run.font.size = size
    if color is not None:
        run.font.color.rgb = color
    run.font.bold = bold
    return run
