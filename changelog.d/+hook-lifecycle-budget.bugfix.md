Lifecycle hooks keep credential lookups and parallel audit workers inside the hook's shared deadline.
Responsive stop audits still reject false completion claims.
Subprocess input delivery is covered by the same deadline when a helper does not read stdin.
HTTP audit deadlines also cover DNS resolution.
Audit input writers report closed pipes without terminating the hook.
