`ljos mesh up` joins the Headscale mesh and `ljos mesh down` leaves it.
The join takes a key file and a hostname, and it does not advertise tags.
Without a usable tunnel device the daemon runs in userspace mode.
A failed join prints one line and exits 0.
