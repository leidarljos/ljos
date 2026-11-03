A push to develop or main gets a continuous integration (CI) run that goes to the end.
A later push no longer cancels it while it waits.
The docs workflow does the same, and main's Pages deploys still run one at a time.
Once its SIGINT handler has reaped a writer, the packsetd test helper's drop sends no SIGKILL to that writer's pid.
