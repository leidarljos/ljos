A tracker git call that runs past a hook's budget, as the user-rules sync at every session start does, is now terminated and given two seconds to clean up before it is killed.
Killed outright in the middle of an add or a commit, git left an empty `.git/index.lock` that refused every later commit on the tracker until someone removed it by hand.
The sync also waits for the tracker's commit lock only until its deadline, and its merges hold that lock, so a session start behind a seat's commit no longer spends its budget waiting and then cuts its own git off.
