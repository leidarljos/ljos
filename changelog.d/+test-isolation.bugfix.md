Parallel runs no longer exchange the process environment, working directory or temporary paths between tests.
A test no longer outlives the fixture it waits on, and a network wait in a test stays bounded.
