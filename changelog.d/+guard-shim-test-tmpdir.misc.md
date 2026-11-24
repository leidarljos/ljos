The guard shim temp-home test takes the environment lock before it makes its directories.
A test that points `TMPDIR` at a directory it deletes could otherwise remove them while the test ran.
