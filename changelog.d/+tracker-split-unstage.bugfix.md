Before it removes anything, a refused board split takes its issue files out of the index. When they stay staged, the split is kept, instead of leaving them staged with the marker gone for the next plain commit to carry.
With no commit yet the files leave the index through `git rm --cached`.
