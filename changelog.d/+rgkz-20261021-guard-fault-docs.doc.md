The integration docs now describe a guard fault as a deny: a policyd
that is installed but hangs, crashes, or prints no verdict, and a
relay that cannot read its input. A missing `ljos-policyd` still
allows, with a note, unless `POLICYD_REQUIRED=1`.

The same docs now document `LJOS_GUARD_FAIL_OPEN=1` as the temporary
escape hatch that restores the previous allow-with-note behavior for
a fault.
