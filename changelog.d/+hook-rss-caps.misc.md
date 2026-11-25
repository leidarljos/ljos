Cap each burst hook call at 8 MB of anonymous RSS, and at 20 MB of VmHWM in a release build, so heap and file-backed growth both fail the hook queue test.
