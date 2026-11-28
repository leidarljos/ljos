A loopback connect that times out stays a timeout when the socket tables cannot be read, so a listener that is up stays busy and is not started again.
It is a refused connection only when those tables were read and nothing is listening on that port.
