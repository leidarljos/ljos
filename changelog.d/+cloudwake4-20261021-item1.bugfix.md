`cloud up` now retires env-owned wakes before writing the cloud age identity,
so a bad `LJOS_CLOUD_AGE_SECRET` cannot leave a stale wake URL that still
sees a rotated wake bearer key.
