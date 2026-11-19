Add `ljos handbook` to write the lessons, preferences, rules, tickets and workflows marked public as one Org document. Nothing unmarked is exported. A marked item that reads as private stops the export. `ljos handbook` then names the item and the kind of match, and writes nothing.
Refuse banned product terms and credential phrasing such as `the password is hunter`, `the pin is 4921`, `curl -u user:pass` and `sshpass -p`, in the public-text screen.
The screen reads an address that follows a letter and a dot, as in `e.g.10.0.0.1`. A private quad inside a longer dotted run is refused, as in `1.10.0.0.5`, and only a public quad written in decimal is spared as a version, as in `v1.10.0.0`.
A `_` beside a number or an IPv6 address does not hide it, as in `_10.0.0.5_` or `_2001:db8::1_`.
The screen reads a numeric internet address as the C library's `inet_aton` does.
Each part may be hex, octal or decimal, of any length, so `00012.0.0.1` and `0x0000000a.0.0.1` are refused.
A private number of one to three parts, or a bare integer, is refused only beside an address word, a port, `//` or `@`, as in `ip 10.1` and `ping 167772161`.
`//` and `@` count on either side of that number, and a letter other than `v` directly before a dotted quad does not hide it.
A handbook line is bookkeeping only when it is a state change of the form `State "DONE" from "TODO"`, a `CLOCK:` line with an org time, or a noted, claimed or finished line that names one seat.
