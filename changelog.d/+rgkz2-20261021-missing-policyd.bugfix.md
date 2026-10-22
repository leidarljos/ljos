A missing `ljos-policyd` still allows the line and says the guard did
not start. `POLICYD_REQUIRED=1` refuses that line, including when
`POLICYD_BIN` names a file that is not there. CI and a fresh install
are not locked out unless that switch is set.
