The pre-push hook exited successfully when LJOS_HOOK_KEY was set,
before it read the allowlist. That variable was only there so a test
could call the URL normaliser, and it shipped in the installed hook.
The normaliser test builds its own script from the functions above
the gate. The installed hook has no off switch.
