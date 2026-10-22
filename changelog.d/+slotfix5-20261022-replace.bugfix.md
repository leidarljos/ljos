`git replace` and a grafts file rewrite commit parents without making
the checkout shallow or dropping an object. Pin order followed that
rewritten parent list, so a pin removed before slots could still
stand. History reads that decide pin order ignore replace refs and
grafts. A checkout that has `refs/replace` or a grafts file is refused,
and the refusal names which one.
