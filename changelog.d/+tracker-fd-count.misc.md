On macOS, which has no `/proc/self/fd`, the tracker lock-wait test counts open descriptors through `/dev/fd` and `fcntl(F_GETPATH)`.
It used to wait out 50 seconds there and fail.
