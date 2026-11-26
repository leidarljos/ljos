Git commands ljos runs no longer start the fsmonitor daemon or auto maintenance.
On macOS that daemon kept a sync test's git child from exiting, and sync runs the same call.
