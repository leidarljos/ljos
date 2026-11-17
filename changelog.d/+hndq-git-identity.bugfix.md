`ljos doctor` now has a `git identity` warning when git has no author name or email, and the row names `git config --global user.name "Your Name"` and `git config --global user.email "you@example.com"`.
When git refuses the tracker commit for a missing identity, the error names those two commands too, where it said only to set `user.name` and `user.email`.
A seat with an identity sees an `ok` row under `ljos doctor --all` and nothing in the short report.
