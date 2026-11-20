A tracker push that fetches and rebases now takes the issues lock of every `issues.org` it can rewrite, a board only the remote has included, after the commit lock, the way every tracker writer does.
A lock it cannot take stops the push before the checkout changes, and a replay that fails is undone before the locks are let go.
A `vissue` or `ljos` write that read the file before the rebase no longer puts back its older copy over the replay, which dropped another seat's ticket from the board.
The merged and folded files are replaced whole, so a reader never sees half of one.
