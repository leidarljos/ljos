The process-state isolation scan now reads each test from the syntax tree, so a comment, a string literal or a similar name no longer counts as a test's guard.
The guard must be a named `let` binding of the guard call in a test's first two statements, and a test attribute the scan cannot read fails it.
