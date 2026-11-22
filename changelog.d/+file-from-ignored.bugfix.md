In a batch across more than one project, `ljos file --from` no longer writes a `Filed by` logbook line into a project file the checkout ignores, since the commit leaves that file out.
Git alone decides what is left out, so a file a `!` rule re-includes gets its line and goes into the commit with the rest.
A tracker commit now takes a tracked file whose directory an ignore rule names; it refused that file before.
