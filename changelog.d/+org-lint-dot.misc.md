The org-lint harnesses load `ob-dot`, so Org 9.8, which maps `dot` to `graphviz-dot-mode`, does not call the dependency graph block an unknown language.
The handbook unit test runs Org 9.8's misplaced-heading check itself on every Org version, with only the backslash that escapes a line starting with stars exempt.
The macOS unit tests set `LJOS_TEST_REQUIRE_TOOLS=1`, and both CI jobs load `ob-dot` when they check the installed Org.
