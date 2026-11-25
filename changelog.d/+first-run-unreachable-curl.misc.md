The first-run check that crates.io did not answer installs the failing `curl` after the registry stand-in.
Before, that stand-in replaced the stub, `ljos doctor` got an answer, and the check found no `crates.io` row.
