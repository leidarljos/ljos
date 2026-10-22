`mail_wake_push` no longer needs Linux user namespaces. The test harness
sets `LJOS_TEST_HOSTNAME` for each simulated machine and runs `ljos`
directly, so CI runners that block unprivileged user namespaces still
exercise push-before-wake behavior end to end.
