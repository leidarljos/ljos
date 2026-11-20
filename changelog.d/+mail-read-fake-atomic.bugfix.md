Write the fake `notmuch` script in `mail_read` to a temp file in a short-lived child process and rename it into place so parallel `fork` calls do not inherit an open write fd and trigger `ETXTBSY`.
