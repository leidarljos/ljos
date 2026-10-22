A `git config include.path`, `--remove-section`, `sed -i`, `perl -i`,
`dd`, or a redirect onto `.git/config` could point `core.hooksPath`
somewhere else. Doctor still read one global key, so the row stayed
ok and the next push skipped the hook.

A write is refused when the path is a gitconfig, whichever tool
names it. Doctor asks git which hooksPath the seat would run. A
throwaway repository's own local hooksPath stays available.
