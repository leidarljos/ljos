The public-text screen reads every IPv4 and IPv6 literal with the standard parsers, at any start and end inside a run of hex digits, colons and dots.
A non-public address is refused next to any letter, digit or separator.
A run of any length is read in linear time, so a long OID, dot leader, hex dump or fingerprint is refused only when a piece of it is an address.
A bare `::` after a separator, at a line start, or as a Rust path still passes.
