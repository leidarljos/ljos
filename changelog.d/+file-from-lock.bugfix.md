`ljos file --from` no longer removes a project's `issues.org.lock` when it takes back a refused batch, so a writer waiting on that lock and the next one to open it cannot both hold it.
It checks each row's write and puts a refused batch back while it holds the project's lock, and stops instead when another writer changed the project directory, so that writer's change is never put back over.
