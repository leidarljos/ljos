The destination allowlist now runs in the pre-push hook `ljos cloud up`
installs. Git runs that hook for every push, including one a script,
make, or another program starts, and passes the remote URL after
insteadOf. The shell guard refuses the switches and the writes that
would skip the hook, and `ljos doctor` reports a hook that is missing
or was replaced.
