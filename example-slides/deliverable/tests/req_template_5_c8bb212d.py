"""Requirement req:template-5 (c8bb212d5fd18e21): deck uses #248555 as primary and #8e4367 as complementary."""

import sys

from pptx import Presentation

PRIMARY = "248555"
COMPLEMENTARY = "8E4367"


def _rgb_value(color_format):
    try:
        rgb = color_format.rgb
    except (AttributeError, TypeError):
        return None
    if rgb is None:
        return None
    return str(rgb).upper()


def _collect_deck_colors(prs):
    colors = []
    for slide in prs.slides:
        for shape in slide.shapes:
            if getattr(shape, "has_text_frame", False):
                for para in shape.text_frame.paragraphs:
                    for run in para.runs:
                        value = _rgb_value(run.font.color)
                        if value:
                            colors.append((value, "text", run.text))
            if getattr(shape, "fill", None) is not None:
                try:
                    fill_color = shape.fill.fore_color
                except TypeError:
                    fill_color = None
                value = _rgb_value(fill_color) if fill_color is not None else None
                if value:
                    colors.append((value, "fill", getattr(shape, "name", "shape")))
            if getattr(shape, "line", None) is not None:
                try:
                    line_color = shape.line.color
                except TypeError:
                    line_color = None
                value = _rgb_value(line_color) if line_color is not None else None
                if value:
                    colors.append((value, "line", getattr(shape, "name", "shape")))
    return colors


def crit_template_5_d26c8d8c():
    prs = Presentation("dist/jazyk-slides.pptx")
    colors = _collect_deck_colors(prs)
    seen = {value for value, _kind, _label in colors}
    assert PRIMARY in seen, "primary color #248555 was not found in the generated deck"
    assert COMPLEMENTARY in seen, "complementary color #8e4367 was not found in the generated deck"

    title_runs = [
        label
        for value, kind, label in colors
        if value == PRIMARY and kind == "text" and label.strip()
    ]
    complementary_marks = [
        (kind, label)
        for value, kind, label in colors
        if value == COMPLEMENTARY
    ]
    assert title_runs, "primary color #248555 is not applied to visible text"
    assert complementary_marks, "complementary color #8e4367 is not applied to a visible deck element"


if __name__ == "__main__":
    selected = sys.argv[1] if len(sys.argv) > 1 else "crit_template_5_d26c8d8c"
    if selected != "crit_template_5_d26c8d8c":
        raise SystemExit(f"unknown test {selected!r}")
    crit_template_5_d26c8d8c()
    print("PASS crit_template_5_d26c8d8c")
