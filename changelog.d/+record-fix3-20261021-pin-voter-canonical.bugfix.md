`ljos record check` now refuses a tracker pin file row whose voter is not spelled the way the check looks it up, instead of silently skipping that pin.
A BOM or zero-width space before `seat-a`, a non-slug seat such as `Seat-a` or `seat_a`, and a malformed `<identity>/<seat>` row all fail the check.
