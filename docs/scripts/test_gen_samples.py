#!/usr/bin/env python3

import unittest

from gen_samples import unwrap_breaks


class UnwrapBreaksTests(unittest.TestCase):
    def test_a_wrap_break_and_its_indent_become_one_space(self) -> None:
        self.assertEqual(
            unwrap_breaks("ljos-consensus is not on PATH. Install it\n  with `cargo install`."),
            "ljos-consensus is not on PATH. Install it with `cargo install`.",
        )
        self.assertEqual(unwrap_breaks("from seat-one\nto seat-two"), "from seat-one to seat-two")

    def test_a_tab_turned_into_spaces_is_caught(self) -> None:
        needle = "source\tLJOS_SEAT"
        self.assertIn(needle, unwrap_breaks("seat\tada\nsource\tLJOS_SEAT\n"))
        self.assertNotIn(needle, unwrap_breaks("seat    ada\nsource  LJOS_SEAT\n"))
        self.assertNotIn(needle, unwrap_breaks("source LJOS_SEAT\n"))

    def test_spaces_inside_a_line_stay(self) -> None:
        self.assertNotEqual(unwrap_breaks("a  b"), "a b")
        self.assertNotEqual(unwrap_breaks("a\t\nb"), "a b")


if __name__ == "__main__":
    unittest.main()
