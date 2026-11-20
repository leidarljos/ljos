A tracker write such as `ljos note` waits up to `LJOS_TRACKER_LOCK_WAIT` seconds, 20 when unset, for another writer's commit lock, as the docs say.
It waited two seconds, while a background push keeps that lock until git exits, so a note during a push was left uncommitted until `ljos sync`.
