An older preference or lesson the upgrade promotes is raised to the
person's again. packset now refuses an update that raises
`source.basis` from none to `person`, so these preferences dropped out
of the session-start list and the upgrade retried every session. The
upgrade now stores a copy as the person's and retires the original.
The copy keeps the words, kind, entities and timestamps, so a rule
that cites the preference still names it. `ljos prefer --review --keep`
takes the same path and prints the new id.

A copy the pack refuses leaves the original as it was. The upgrade
goes on to the rest, writes the refusal on its record, says so once on
stderr, and does not send it again.
