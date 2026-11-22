`ljos file --from` refuses a project whose `issues.org` or `issues/` is a symbolic link before it writes anything, records any other link it finds there by its target, and never removes or writes through a link when it takes back a refused batch.
Before, a linked `issues.org` was replaced by a plain file, taken as the batch's own create, and then deleted.
