The job-folder test no longer exports `GIT_DIR` for the whole process.
The origin lookup still receives that directory on its git command and drops it, so `-C` stays on the checkout.
