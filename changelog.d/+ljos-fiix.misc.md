Each push to develop or main gets a CI run of its own that runs to the end, so a later push no longer cancels it while it waits.
The packsetd test helper no longer sends SIGKILL to a writer's pid once its SIGINT handler has reaped that writer.
