A tracker `index.lock` more than a minute old, with no git process working in the checkout, is now named when a commit meets it: its path, its age, that a killed git left it, and the `rm` that clears it.
Every later commit used to fail on it with git's `File exists` after six seconds of retries, and the hooks that commit in passing failed without a word.
