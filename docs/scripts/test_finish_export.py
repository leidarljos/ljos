#!/usr/bin/env python3

import pathlib
import tempfile
import unittest

from finish_export import _crate_version, _sentences, mismatched_ljos_versions


class FinishExportTests(unittest.TestCase):
    def test_the_version_sentence_survives_the_description_cap(self) -> None:
        prose = (
            "A plan is how a seat opens a day's work: one parent, an ordered checklist, "
            "and the next unfinished item. "
            "This page installs ljos, stores a lesson, opens that plan, and puts a second seat on a ticket. "
            "It describes ljos 0.46.0."
        )
        self.assertGreater(len(prose), 220)
        self.assertIn("ljos 0.46.0", _sentences(prose))

    def test_a_different_ljos_version_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            page = root / "docs/source"
            page.mkdir(parents=True)
            (page / "note.rst").write_text("It describes ljos 9.9.9.\n", encoding="utf-8")
            bad = mismatched_ljos_versions(root)
            self.assertEqual(len(bad), 1)
            self.assertIn("ljos 9.9.9", bad[0])
            self.assertIn(_crate_version(), bad[0])

    def test_the_crate_version_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            page = root / "docs/orgmode"
            page.mkdir(parents=True)
            text = f"It describes ljos {_crate_version()}.\n"
            (page / "note.org").write_text(text, encoding="utf-8")
            self.assertEqual(mismatched_ljos_versions(root), [])


if __name__ == "__main__":
    unittest.main()
