import unittest
from pptx import Presentation
from pptx.enum.dml import MSO_THEME_COLOR

class TestTemplate2(unittest.TestCase):
    def setUp(self):
        # The build must have run successfully and produced dist/jazyk-slides.pptx
        self.filepath = "dist/jazyk-slides.pptx"
        try:
            prs = Presentation(self.filepath)
        except FileNotFoundError:
            self.fail("The artifact dist/jazyk-slides.pptx was not found after the build.")

    def test_deck_is_powerpoint_format(self):
        # This is a basic check that we successfully loaded the file, implying it's a valid PPTX structure.
        self.assertIsInstance(prs, Presentation)

if __name__ == '__main__':
    unittest.main()