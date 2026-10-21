Sitting and finish under `LJOS_TRACKER_GIT=commit` no longer reach the network for the pack log.
Sitting used to fetch every tracker remote and then read and post the pack, and a pack that did not answer in time printed `3: timed out reading response`.
Finish still commits the sealed log when it changes, and it says the log was not pushed, including when the log was unchanged and a tracker commit was waiting to be sent.
`ljos sync` run by hand still pulls and pushes.
