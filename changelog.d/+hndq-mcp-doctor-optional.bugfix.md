`ljos_doctor` over `ljos mcp` now marks each optional row with `optional` true, so an absent `packset-embed`, `ljos-hud`, cloud guard, runners or git hooks row with `ok` false no longer reads as a failure.
The encoder install command in that row, and in `ljos doctor`, no longer changes with whether `ljos mcp` started in a checkout with an `install.sh`.
