A process that leaves its runner with setsid, a double fork, script or a detached tmux still counts as under that runner when its cgroup, or on macOS the responsible process, names one.
A keep and a prefer from that process stay the runner's.
Leaving the cgroup, or handing the process to launchd, still gets past the check. A cleared environment does not.
