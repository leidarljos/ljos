The handbook and workflow screens now classify the same canonical text.
They decode one layer of percent, HTML, `\u`, `\x` and quoted-printable escapes.
They normalize line breaks and spaces and fold allowed symbols to plain keyboard characters for detection.
They find a path, an email address or a secret cut by an ordinary line wrap.
Addresses are found within one line.
An address broken across lines on purpose is not caught.
The screens refuse addresses in `10.0.0.0/8`, `127.0.0.0/8`, `169.254.0.0/16`, `172.16.0.0/12`, `192.168.0.0/16` and `100.64.0.0/10`.
Every private dotted quad is refused.
A public dotted quad is refused unless it is a resolver, a netmask or a version-context quad written in decimal.
A netmask is ones then zeros, as in `255.255.240.0`, so `255.0.255.0` is refused.
Version context is a version word before or after the quad, or a `v` glued to it, as in `v1.10.0.0`.
A private number of one to three parts, or a bare integer, is refused only beside an address word before or after it, a port, `//` or `@`, as in `127.1 host`.
A backtick or verbatim span does not hide an IPv6 address such as `::a`, and an IPv6 address after a dot, as in `x.::a`, is refused.
