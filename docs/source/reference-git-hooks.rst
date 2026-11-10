:description: The global git hooks that book commits, and how to install, remove and check them.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========
Git hooks
=========


``ljos git-hooks install`` writes a stub for every standard git hook and sets the global ``core.hooksPath`` to that directory.
The directory is ``$LJOS_GIT_HOOKS`` when that is set, otherwise ``$XDG_DATA_HOME/ljos/git-hooks``, otherwise ``~/.local/share/ljos/git-hooks``.
It prints ``installed`` and the directory.
A ``core.hooksPath`` that was already set is saved as ``previous-hooks-path`` in that directory.
When ``core.hooksPath`` still names that directory, ``ljos git-hooks uninstall`` puts the saved path back, or unsets it. It then removes the stubs and prints ``uninstalled``.
``ljos git-hooks status`` prints ``hooks:`` and the directory, or ``not installed``, then ``core.hooksPath:`` and the configured value, or ``(unset)``.
``ljos onboard --harness example-host`` installs the same hooks.

Each stub chains to the repository's ``.git/hooks/<name>`` when that file is executable, otherwise to the saved path's hook of the same name, and keeps that hook's exit status.
``post-commit`` and ``post-rewrite`` also run ``ljos hook --git-commit --git-hook NAME``.
Booking never fails the hook. A line it prints goes to stderr, and a habitat that did not answer is a line in the book log.
``LJOS_OFF`` set to ``1``, ``true``, ``yes`` or ``on``, in either case, skips the booking, and the stub still chains.

``ljos hook --git-commit`` books the commit git just created.
``post-commit`` reads no stdin.
``post-rewrite`` reads the old and new shas and moves the citation onto the new commit. A dropped commit, an all-zero new sha, loses its citation.
``--git-hook`` is ``post-commit`` or ``post-rewrite``.
An amend or a rebase moves the deed onto the new sha and does not leave the replaced sha cited.
The sitting's lesson is written again for the new sha and closes the one that named the replaced sha.
A ``post-commit`` in the middle of a rebase or a merge does not book. ``post-rewrite`` does.
A commit already cited on an open sitting in that checkout is not opened as a second book.

The hook books as a seat. The holder is ``SEAT@TAG``, the same name ``ljos claim`` uses for that conversation.
``LJOS_SESSION_ID`` alone is kept as written.
``LJOS_SEAT`` wins when it is set and is not ``seat``, ``you``, ``agent``, or a name that slugs to ``runner``.
Otherwise ljos derives the name from the markers the runner leaves in the environment, and a derived name is a seat.
When nothing named the seat, an ``ANTIGRAVITY_CONVERSATION_ID`` books as ``antigravity`` and an ``OPENCODE_SESSION_ID`` books as ``opencode``.
A terminal harness or the command harness session, named by ``CLAUDECODE``, ``CODEX_THREAD_ID`` or ``CODEX_SESSION_ID``, is not booked.
An opencode session is booked by this hook, including a commit from its shell or a subprocess.
An agy session is booked the same way when no seat was named.
Without these hooks, ``Stop`` and ``SessionEnd`` cite each commit HEAD's reflog says the checkout wrote since the sitting began, so a commit from a script, or one chained before a step that failed, still has a deed.
A pull, a merge or a checkout adds none.

The env file ``ljos onboard`` writes sets ``LJOS_GIT_HOOKS`` to ``${LJOS_GIT_HOOKS:-${XDG_DATA_HOME:-$HOME/.local/share}/ljos/git-hooks}``, the same directory the hooks install into.
``ljos doctor`` reports an env file that sets a different directory.

Commits under ``[git_hooks] skip`` in ``ljos.toml`` are not booked, and neither are ``/tmp``, ``/var/tmp``, ``/dev/shm``, the tracker (``VISSUE_ROOT`` or ``ISSUE_ROOT``), ``CLAIMDAG_DIR``, and the deedar store.
The table is read from ``$LJOS_CONFIG``, from ``./ljos.toml``, from ``~/.config/ljos/ljos.toml``, and from ``~/.config/ljos/seats/$LJOS_SEAT/ljos.toml``.
A reinstall rewrites the hook scripts and leaves that file alone.
``examples/ljos.toml`` shows the table.
