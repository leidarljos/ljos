A move capture is read from the raw tokens of the test, so a borrow, a call or a macro around the closure still ends the guard.
A parameter of that closure does not.
