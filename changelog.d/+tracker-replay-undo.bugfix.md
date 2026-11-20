A tracker push whose replay cannot be undone, because every `git rebase --abort` fails, now says the checkout is still mid-rebase or mid-merge and what clears it.
A tracker commit in a checkout that is mid-rebase or mid-merge is refused, instead of going into the stopped rebase.
A push that finds the checkout already mid-rebase or mid-merge now changes nothing and says so, instead of aborting over a note written there.
