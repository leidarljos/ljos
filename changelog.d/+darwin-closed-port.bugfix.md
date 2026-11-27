A connect to a port that is bound and not listening is refused on Linux and times out on Darwin, because Darwin drops the SYN.
Loopback connects on Darwin stop at 400 ms unless `LJOS_PACK_CONNECT_MS` is set, and that handshake timeout is read as a pack that is not running.
