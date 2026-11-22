Test seat roots and the scratch git ceilings now come from the `HOME` the test process booted with, symlinks resolved. A test that points `HOME` at a temp tree no longer moves them for tests running beside it.
The guard shim test and the login-home user-rules test now take the environment lock before they make a seat directory.
