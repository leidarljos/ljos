#!/usr/bin/env python3

import unittest

from gen_samples import listening_port, unwrap_breaks


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


class ListeningPortTests(unittest.TestCase):
    def test_a_line_cut_before_or_inside_the_port_is_not_read_yet(self) -> None:
        for partial in (
            "writer: listening on http://",
            "writer: listening on http://127.0.0.1",
            "writer: listening on http://127.0.0.1:",
            "writer: listening on http://127.0.0.1:87",
        ):
            self.assertIsNone(listening_port(partial), partial)

    def test_a_whole_host_and_port_is_read(self) -> None:
        self.assertEqual(listening_port("writer: listening on http://127.0.0.1:8765\n"), 8765)
        self.assertEqual(listening_port("listening on http://localhost:41234/ (ctrl-c)"), 41234)


if __name__ == "__main__":
    unittest.main()
