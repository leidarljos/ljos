# Changelog

Versions follow semver at 0.x: a minor bump is a feature, a patch is a fix.
Terms used below: Model Context Protocol (MCP), trusted computing base (TCB), heads-up display (HUD), software bill of materials (SBOM), European Environment for Scientific Software Installations (EESSI).

<!-- towncrier release notes start -->

## 0.34.0 (2026-10-27)

### Features

- A runner's push onto `develop` or `main` also waits for the seats on
  the ticket it sits on: a quorum of seat ballots, two seats by default,
  from two model families, deciding an option that does not send the
  work back. The refusal names `ljos consensus ID --no-file`, whose new
  `protected push:` line says whether the ticket clears the push. A push
  onto the seat's own topic branch needs only the sitting.
- The `pre-commit` and `pre-push` hooks `ljos git-hooks install` writes
  run the sitting gate too, so a commit or push a script makes under a
  runner is refused the same way, and `pre-push` reads where each ref
  lands from what git sends it.
- Under an agent runner, the guard refuses `git commit`, `merge`,
  `rebase`, `cherry-pick`, `revert`, `am`, `pull` and `push` until the
  conversation holds a live sitting on an open ticket, and the refusal
  names `ljos sitting ID`. The person at their own terminal is not
  gated, and no variable turns the gate off.
- `ljos doctor --all` has a hook latency row: the 95th percentile of gate calls and of their wait for a slot, how many calls the queue refused, and the 95th percentile wait for `book.lock` over the book jobs. The hooks write these to `hook-times.log` in the runtime directory, cut back past 256 KB.
- `ljos doctor` has a `sitting gate` row: under a runner it names the
  seat, the sitting it finds and the quorum, and warns when the settings
  do not read; on the person's machine it is `info` and fails nothing.
- `quorum` and `consensus_branches`, under `[gates]` or at the top of the
  user or seat `ljos.toml`, set how many seats a protected push needs and
  which branches are protected. The repository's `ljos.toml`,
  `LJOS_CONFIG` and `.ljos/gates.toml` may only raise the quorum and add
  branches.

### Fixes

- A 400 refuses `rank=relevance` only when the body names that parameter
  or carries a JSON field of that name. A sentence that merely uses the
  word no longer makes the writer look like it refused the rank.
- A Claude plugin installed under the home .claude directory names the seat when its server command is the seat.
- A Cursor `beforeReadFile` whose body is large answers allow before that body is parsed. The trace marker still keeps the full payload.
- A Cursor `preToolUse` for a seat tool named `mcp__SERVER__ljos_NAME` prints allow instead of an empty answer.
- A Cursor hook whose `conversation_id` is a blank string now reads the conversation from the same environment order the CLI uses, with `CURSOR_CONVERSATION_ID` in that place. The hook call's session becomes that id, so the sitting book is touched the way a payload that carries the id touches it. A non-empty id that names nobody is still refused.
- A Cursor hook whose payload and environment both name no conversation no longer waits for a sitting. Empty or missing conversation, session and generation ids, with a null transcript path, are recorded as unattributed in the book log, and a history write, including a push onto develop or main, is allowed. The command checks still refuse `rm -rf` outside a temp directory and the other dangerous commands. A payload that names a conversation is judged as before, and a non-empty id that names nobody is still refused.
- A PreToolUse allow for `mcp__SERVER__ljos_NAME` requires `SERVER` to be a key whose command is the seat MCP server. The call is still recorded for the subagent when that key is not configured.
- A `beforeMCPExecution` whose stdin is not UTF-8 prints an answer on the direct path as well as the queue path.
- A bare seat whose conversation could not be bound may not take a hold through `ljos_claim` either; only `ljos_sitting` was refused. The bare hold was adoptable by the first conversation to ask, and could take over a live tagged conversation's hold on the same ticket.
- A cloud seat whose protected push is missing only a seat ballot now asks, and the hook records a one-use request for that exact command. The push still needs a live sitting. A force, a delete, a cursor ref, and a repository the allowlist does not name stay refused. A call that names no conversation is still refused on a protected branch.
- A command is the person's only when they are at the terminal and the process is not under a runner. The runner check now includes a cgroup that still names one and a transient `run-*.scope`, which is what `systemd-run --user --scope` and a D-Bus scope leave after the environment has been cleared, so a detached runner with a terminal is not the person.
- A command key in tool input does not replace the MCP: tool name, so a user-rule pattern for that tool still matches.
- A controlling terminal was enough, once no runner mark was set. `systemd-run --scope`, a container with a cleared environment, and a CI runner (`GITHUB_ACTIONS`, `Runner.Worker`) could all open a pty and be stored as the person. The person is present only when that tty belongs to an interactive login session: a terminal, a multiplexer or a login holds it. A transient scope, a container and a CI runner are not that session.
- A curl URL whose glob spans the scheme, `{https://api.github.com,https://example.com}/...` or `http{s://a,s://b}/...`, may reach GitHub, so a write to one is refused unless `-g` is given. The authority split read the host after the first `://`, which the braces hid.
- A curl or wget to an IP address, in any form the clients take, was
  judged by the address, which is not a GitHub name, so a POST with a
  `Host: api.github.com` header was allowed. An address host may be
  GitHub now.
- A curlrc holding only an ordinary option such as `tlsv1.2`,
  `http1.0`, or `continue-at` refused every curl, a GET to GitHub and
  a POST to another host included, because any option the guard did
  not know was opaque. The TLS, HTTP version, auth, retry, range,
  trace, and output options are known and harmless now, on the command
  line and in a curlrc. The options that move the target stay parsed.
- A hook call no longer keeps the book inside the call, under the `book.lock` every seat on the host shares. It leaves a job in the runtime directory and answers, and one drainer per user, `ljos hook --drain`, applies the jobs in the order they came. A session start or prompt hook still names the claimed issue, read from the book without its lock. `ljos sitting`, `claim`, `slot`, `finish`, `release` and `complete`, the MCP server and the git commit hook first let the jobs queued before them apply, for at most 5 s.
- A hook call that names no conversation may not push onto a protected branch. Such a call skipped the sitting gate whole, so any runner whose payload carried blank ids could push `develop` or `main` with no sitting and no vote. A commit, a merge and a push onto a topic branch from such a call are still allowed and logged as unattributed.
- A hook call waits at most 1 s for `book.lock`, which every seat on the host shares. When another process keeps it longer, the call keeps no book and the book log says so, instead of waiting behind a holder that is filing a claim or committing the tracker.
- A hook check of one conversation no longer takes the MCP binding record another conversation's server is about to read. Adopting a bare-seat hold read the newest record by consuming it, so a shell call from conversation B could delete the record conversation A had just written, and A's next bare `ljos_sitting` was refused.
- A large ~/.claude.json still names the seat. The hook reads the top-level mcpServers value and stops.
- A listing row with no origin, a null one or an empty one now overwrites a person recorded earlier in the process with an agent. The cold prompt, issue recall and `ljos recall` no longer hand a preference over as the person's when an older packsetd lists it without its origin.
- A ljos_ tool on a GitHub server is a GitHub write, so a rules file that cannot be read denies it.
- A missing working directory does not deny an MCP tool call.
- A plugin launcher whose argument is ljos-mcp is the seat server, so that entry answers early.
- A pre-tool hook reads the pack's rules and preferences by kind, each within 400 ms. When the pack does not answer in time the call decides from the rules kept on disk, without memory recall, and the hooks of the next 30 s do the same without asking, so a hung `packsetd` costs one short wait instead of one per tool call.
- A preToolUse tool name of the form MCP:<tool> is the line a user rule pattern matches.
  A write that carries owner and repo is checked as a GitHub write.
- A preference migration replays a pending successor noted on the cutoff receipt only when this machine journaled that note, id and body, by hash in its local cutoff record.
  A note written onto the receipt by any other process neither closes the atom it names nor posts its body.
- A preference or lesson in the old terminal shape is not promoted when its harness or session word names a runner, compared without case against the list runner detection uses, so harness `claude` with session `claude` stays unverified.
- A preference whose `valid_to` has passed, or a tombstone, is no longer shown as the person's while its review is still due. The session preface, the cold prompt, issue recall and the search labels read it as not held, as the pack's by-id read does.
- A prompt longer than 8000 characters was kept truncated, so it no longer matched the runner's transcript as a whole entry and the person's words were treated as the agent's. The bind now also stores a hash of the full prompt. Agreement requires that hash and the kept prefix, so a short clause substituted for the prefix still does not match.
- A queued write files its claim with `book.lock` let go. Filing reads the tracker, and every other bookkeeper on the host used to wait for it.
- A request already written to `ljos-mcp` is parsed before stdin is
  treated as closed. The client watch no longer polls that fd itself:
  tokio reads it on another thread, so a hangup there can show up
  after the bytes have left the pipe and before they are a message.
- A rewrite of user-rules.json does not deny a benign GitHub write.
  An unchanged bad file for 80 ms is corrupt.
  A file whose bytes keep changing is re-read until it parses or 200 ms pass, and then the read fails closed.
- A runner variable left in an interactive terminal hid the person even when no runner was in the process tree or the scope, so a command typed there was stored as the agent's. The variable now hides the person only together with that evidence, or when there is no terminal.
- A search hit, an island row or an activate row that puts the person's id on other words is no longer labelled the person's. Each origin is kept with the kind and text of the record it was read from, and a row whose kind or text differs is an unverified agent note in the prompt, `ljos search`, `ljos island`, the timeline and MCP.
- A search that keeps answering 401 or 403 still falls back to stored
  memories, and the hook now says so on stderr at most once a minute,
  naming `ljos doctor`. Doctor reports the refusal as a warning, and a
  seat whose search answers is unchanged.
- A search whose listener asks for a token, met by a seat that has none or
  cannot read its token file, no longer reads that listener without a
  token. The recall is the stored atoms alone, and the miss is said.
- A seat key is read only from the seat's own config. A project MCP file, CLAUDE_CONFIG_DIR, CODEX_HOME, and a parent's --mcp-config do not name the seat. The Cursor payload command, when present, has to be the seat server.
- A second top-level `hook_event_name` or `cursor_version` is not answered as a file read.
- A server key is kept in Claude Code's tool-name form, with every other character replaced by `_`, beside the raw key.
- A shell hook arms its eight second deadline before it takes tmux-sock.lock.
  The take is a try-lock that retries for a short budget, and that budget stays inside the deadline.
  When the lock is not free in time, the pane lookup is skipped and the call is judged as if no pane were found.
- A side event stops waiting on a quiet pack and keeps the note it already has. A spent budget returns before the next socket is opened.
- A symlinked `~/.curlrc`, `~/.wgetrc`, or `wget --config` file was
  treated as unreadable, and an unreadable default config was ignored
  when the command line showed a host that is not GitHub. Curl and wget
  follow the link, so a `url` or `input` in the target reached GitHub.
  The guard now follows the link and judges the file when this user or
  root owns it. Any config the client may load that the guard cannot
  read, and a wgetrc line it cannot parse, leave the target unknown.
- A terminal-shaped preference or lesson is promoted only when both its `ts` and the pack's own `updated` stamp are before the migration cutoff, so one posted after the cutoff with a backdated `ts` stays unverified and is listed for review.
- A tmux pane is bound to the conversation that spawned it only when an ancestor of the call runs the spawn's script as given; a substring match bound a pane whose ancestor was any `bash` to a record that said `bash run.sh`. A scripted spawn that matches no ancestor binds nothing, where every record naming one id was taken as the pane's. A pane pin names the pane's shell by pid and start time, so a tmux server restarted at the same socket, whose panes count from `%0` again, does not inherit the old pins.
- A tool call could `setsid -f python3` (or node, or perl) so the process above `ljos prefer` was not a shell, and the runner's cgroup was never read. The cgroup is read whatever the ancestors are. An unreadable `/proc/self/cgroup` counts as a runner, so a hidden scope does not become the person.
- A tool call on a tracker that names no seats no longer writes a `seats` list into `vissue.toml`. That write left an untracked file in the checkout, re-serialised a file the person wrote, and listed only the first seat to connect, which refused every other seat afterwards. The call sits as the seat it named, with a note in the book log; the roster stays the person's decision in `vissue.toml`.
- A tracker write now carries only the seat its check named: `file` and `open-panel` create their issue as that seat even when `VISSUE_AGENT` names another, `predict` and `slot` sign as it, and a conversation id `seats.toml` maps votes as that seat instead of being refused.
  `--as` on a ballot, a withdrawal or a forecast may name only that seat; another seat is refused on the command line and over MCP, and each decision-panel member is seated as its persona.
  `open-panel` and `bump-plan` (without `--dry-run`) go through the seat check, so an unnamed or unlisted seat opens no panel and files no module issues.
  The seats file is read only from the ljos config directory: `LJOS_SEATS` no longer points the check at another file, and a seats file that links to nothing refuses every write.
  A commit made in the tracker's own checkout gets the seat check too: the git hook says, and logs, when that seat may not write, and ljos's own tracker commits run as the seat that wrote.
  Under an agent runner the guard (`ljos hook`, `ljos policy`) refuses a raw `vissue` write (`create`, `q`, `refile`, `note`, `append`, a ballot `vote`, `claim`, `release`, `close`, `cancel`, `update`) and names the ljos verb to run instead; reads, the person at a terminal and the `vissue` ljos runs itself are not refused.
  `ljos file --parent P` files the child in P's project even when `-p` names another, and `ljos finish --status cancelled --close` closes the ticket as CANCELLED.
  `ljos doctor` warns on the tracker row when issue files in the tracker checkout hold uncommitted edits.
- A user-rule deny on `beforeMCPExecution` exits 0 with the deny JSON. Exit 2 stays on a hook failure, so the runner still shows the reason.
- A wget line with a visible non-GitHub URL and a second operand the shell has not expanded, `"$URL"`, may reach GitHub: wget requests every operand on its line. The hidden operand was dropped once any other host was visible.
- An MCP call is recorded before the early answer, so a bare seat can bind the conversation that invoked ljos_sitting.
- An `ljos-mcp` whose client has gone, and whose stdin a launcher still
  holds open, exits once it has been quiet and no editor or agent is
  still above it. `LJOS_MCP_ORPHAN_IDLE`, or `[mcp] orphan_idle` in
  `ljos.toml`, sets that quiet window. A bare number is seconds. The
  default is six hours. `0` or `off` leaves the server until the client
  or stdin ends it.
- An ask whose hook payload names no conversation now records a one-use request for that command and directory, and the refusal prints its id.
  The request is stored with no conversation, so a chat reply cannot grant it and it is not attached to another conversation.
  The person grants it with the ljos_request_approval form or `ljos approve ID`, and the same command then passes once from any caller that also names no conversation.
- An empty user-rules file and a dangling symlink deny a covered GitHub write and name the path.
- Curl and wget percent-decode a URL host before they connect, so a
  write to `https://api%2egithub%2ecom/user` reached GitHub while the
  guard read a name that is not GitHub's. The guard now decodes the
  host until nothing changes before the GitHub check, on the command
  line, after `--url`, and in a curlrc `url` line. An encoded `@` or
  `:` splits the userinfo or the port, and a trailing dot is the same
  host.
- Curl expands `{a,b}` and `[1-3]` in a URL, so a POST to
  `https://git{hub,xxx}.com/` went to `github.com` while the guard read
  one host that is not GitHub. A glob in a URL host on the command line
  or in a curlrc now leaves the target unknown, unless `-g` is given.
- Curl options on the command line that send a request somewhere the
  URL does not say were skipped: `--resolve`, `--connect-to`, a proxy,
  a `Host:` header, a DoH URL, a unix socket, and any option the guard
  did not know. A POST to another host could reach GitHub. The command
  line and a curlrc now read options through one table, abbreviations
  and `--no-` included, and each of those leaves the target unknown.
- Every IPv4 and IPv6 literal counted as a possible GitHub peer, so
  `curl -X POST http://127.0.0.1:8761/x` and `curl -X POST
  'http://[::1]:8761/x'` were refused while the same write to
  `localhost` passed. A curl or wget write straight to `::1` or to
  `127.0.0.0/8`, in any form the clients take, passes now. A proxy,
  `--resolve`, `--connect-to`, `-x`, a `Host:` header, or a redirect
  still refuses it, and wget needs `--max-redirect=0`, as it does for
  `localhost`. A `[` that opens an IPv6 address is no longer read as a
  curl glob. HTTPie keeps loopback refused, and every other address
  stays refused.
- Every tracker write now records the seat it checked: runner hooks and git-hook bookings, `workflow`, `consensus`, `playbook`, `reap --apply`, `vote` (including `--withdraw` and `--jev`), `predict` and `slot`, on the command line and over MCP, go through the same seat check as `note` and `claim`. An `--assignee` naming another seat is refused, except `release --assignee`, which must name a listed seat. The seat is resolved as `ljos seat` resolves it, so a conversation id `seats.toml` maps writes as that seat, and doctor's `seat source` row agrees with its `seat` row. A seats file that lists no seat refuses every write instead of allowing all, and `seats = [...]` is read as the list.
- HTTPie with no method sends POST when `--raw` gives a body or stdin is
  not a terminal, but the guard judged `http --raw '{}' URL` and
  `echo '{}' | http URL` as GETs and let them reach GitHub. A `--raw`
  in any unique prefix, a pipe into `http`, and a `<` redirect,
  here-document, or here-string now make the request a write unless
  `-I` or `--ignore-stdin` is given. A request item is a body only when
  its first separator is `=`, `:=`, `=@`, `:=@`, or `@`, so a `==` query
  or a `:` header no longer is.
- On macOS a typed command is the person's when the terminal is the controlling one and the process the kernel holds responsible is not a runner. If that process cannot be named, the check fails closed. On any other host that is not Linux there is still no check, and `ljos doctor` says attribution is absent without failing the seat.
- On macOS the parent walk and the responsible-process lookup ran `ps` from `PATH`, so a caller could name that program. Both use `/bin/ps`. Harness names match without regard to case, so `Cursor` is `cursor`. A missing responsibility symbol or a pid of 1 is not proof that no runner is there. A process `launchctl submit` handed to launchd, with no terminal, is not the person.
- One MCP file that cannot be read hides only its own keys.
- Session start syncs user rules on the tracker budget, so a slow fetch still brings a rule written on another machine and the pack is still asked.
- The MCP server's orphan check knows every client the installer onboards: copilot, qwen, droid, crush, kiro, hermes, omp and cline. A session under one of those left idle past the orphan timer lost its server while the client still held the pipe.
- The book drainer and the hook slot server exit once the ljos binary under their path is replaced, so an install takes effect on the next hook call. A drainer that outlived an install spawned the old path for every batch, failed, and dropped each queued claim, note and finish after twenty tries; a slot server kept whatever limits and protocol it started with until the host was idle for fifteen minutes. A batch that cannot start is now named in the book log and stops the drainer instead of being retried against the same error.
- The book drainer commits the tracker once per tracker file for each batch of jobs, naming every write in the message, instead of once per claim.
- The command guard refuses `ljos prefer --review --keep` from an agent's
  tool call, as it refuses `ljos approve`.

  The upgrade refuses a raise, and closes nothing, when another live
  record makes the same claim and would answer for the person's copy.
  Only a record with the original's entities and timestamps, born with a
  person basis, counts as the copy.
- The git hooks' sitting gate exempts a tracker checkout only when it holds `vissue.toml` or a `Software/<project>/issues.org`, and a store only when the store settings name it. A bare `Software/` directory, or a checkout path that merely contained `claimdag`, skipped the gate.
- The guard read `$XDG_CONFIG_HOME/curlrc`, which curl never loads, and
  skipped the files curl 8 does load: `$XDG_CONFIG_HOME/.curlrc`, and
  with `XDG_CONFIG_HOME` unset, `~/.config/curlrc`. A `request = DELETE`
  there left a curl to GitHub allowed, and a write to `~/.config/curlrc`
  was not refused. The guard now walks curl 8's order, lets a missing
  `$CURL_HOME/.curlrc` fall through as curl does, reads no Windows file
  on Linux, and refuses a write to any file in that order.
- The guard reads a curl or wget config before the client opens it
  again, and a write to `SYSTEM_WGETRC` or to a config named with
  `-K` or `--config` was not refused, so the same line could change
  the file in between. Those writes are refused now, and a curl or
  wget that only reads a protected config is no longer taken for a
  write to it. A process already running can still change a file
  between the two reads.
- The guard reads a wgetrc before wget does, and a `WGETRC` or
  `SYSTEM_WGETRC` set on the line was not a protected write target, so
  `printf 'input = https://api.github.com/user\n' > f && SYSTEM_WGETRC=f
  wget ...` was judged from the file as it was before the line ran. A
  path the line sets in either variable, as a prefix, an `env` or
  `export` argument, or a bare assignment, is protected now, so a write
  to it on that line is refused. A relative `SYSTEM_WGETRC` is read
  from the command's directory, and a missing one still means
  `/etc/wgetrc`, as wget reads it.
- The guard's fault budget is eight times a measured policyd spawn, and at least two seconds. A fixed 200 ms was shorter than a slow CI spawn, so an allow was read as a timeout. A policyd that sleeps past the budget is still a fault.
- The macOS build of the client watch compiles. The argument count
  read from the kernel is an `Option`, and the kqueue timeout is not
  mutated.
- The mail checks fold away, and export refuses, every Unicode
  Default_Ignorable_Code_Point character rather than a hand-kept list, so a
  denied term split by a variation selector (U+FE00-FE0F, U+E0100-E01EF) or
  a tag character (U+E0000-E007F) is caught. Shown mail replaces them with
  U+FFFD.
- The mail reader drops more concealed HTML: entity-encoded or
  comment-split styles, a `/` before an attribute, percentage opacity,
  sub-pixel fonts, zero alpha and off-screen boxes. It withholds a part
  whose stylesheet colour can meet a background, and reads `@media` and
  `@supports` rules instead of withholding them. A `multipart/alternative`
  whose plain and HTML text differ shows both, labeled, with
  `alternatives_differ` set. The checks scan a draft body as written and
  as a client would render it, so `<https://...>` passes. Drafts must be
  7bit ASCII with lines of at most 998 octets, and export refuses every
  `Resent-*` header. The user mail config, `deny_file` and
  `notmuch_config` are opened `O_NOFOLLOW` and refused when any directory
  above them is open to other users. Invisible format characters and the
  line and paragraph separators are refused in drafts and replaced in
  shown mail.
- The mail reducer no longer panics on an `&` followed within twelve bytes by a multibyte character, reads `<!-->` and `<!--->` as the whole comments HTML5 makes them instead of swallowing the rest of the part, and reads at most 1 MiB of a part, reporting the rest hidden, since a part of unterminated tags cost the square of its length.
- The pack token stays in one client and is written only after the address the writer's URL resolved to has refused an unknown atom twice. The request uses that socket, ignores a proxy and does not follow a redirect. packsetd closes the socket, so the next call proves again instead of trusting a proof remembered for the process.
- The pack's listener is proven once per process and origin before any read that names the person carries the token. The proof asks for an atom no pack holds with no token and with a bogus one, which both have to be refused, then with this seat's. The session preface, the cold prompt, the guard's preferences, issue recall, `ljos search`, MCP `ljos_search` and the search fallback listing all wait for it.
  A listener that refuses only some URLs, or does not check the bearer, gets no token and supplies only an agent's memory.
- The person at a terminal inside tmux, screen or zellij, or logged in through OpenSSH 9.8 or later, is the person again. The login check compared the holder's process name whole, so `tmux: server` and `sshd-session` matched nothing, and a daemonised multiplexer counted as an adopted process; `ljos prefer`, `ljos approve` and the sitting gate then treated the owner as a runner.
- The person check no longer passes with a program between the terminal
  and ljos (`python`, `node`, `make`, `expect`), or on a terminal whose
  holder init adopted, the shape a double fork and a second `setsid`
  leave. Both let an agent's tool call keep an older preference as the
  person's. A multiplexer server started over ssh is adopted the same
  way, so type the command in the login shell there.

  The preference upgrade writes its cutoff and promotes older preferences
  only for the person at a terminal of their own, the check
  `ljos prefer --review --keep` makes. `ljos doctor`, `ljos prefer` and
  `ljos prefer --review` run any other way, such as with stdin closed,
  only report, and the review says where to run it.

  Two upgrades at once, such as `ljos doctor` beside `ljos prefer`, no
  longer lose an older preference. They take turns on a lock file in the
  state directory while one notes, copies and retires a preference. When
  the pack refuses the copy and then misses the reopen, the note stays
  and the preference is not marked refused, so the next run reopens it.

  The seat guard refuses typing `ljos prefer` or `ljos remember` into a
  pane, not only an approval: `tmux send-keys`, `screen -X stuff`,
  `xdotool`, `wtype`, `ydotool`, `ttyecho`, `writevt`,
  `kitty @ send-text`, `wezterm cli send-text`, herdr and zellij. Typed
  into the person's pane they ran at the person's terminal and stored an
  agent's words as theirs. A typer fed from stdin or a file, and a
  program that pushes keys with `TIOCSTI`, are refused outright.

  An older preference stored without `valid_from` is promoted once. The
  pack fills `valid_from` on the person's copy, and the upgrade took that
  copy for another record, reopened the original beside it and marked it
  refused.
- The preference cutoff recorded from a receipt the pack already held is the oldest stamp on that receipt. `updated` was read first, and it moves on every rewrite of the receipt, so a re-recorded cutoff could land after a seat-token post and promote it.
- The preference migration cutoff is recorded in the person's state directory when a person-side run first migrates, and only the pack receipt that record names is a cutoff.
  A cutoff receipt posted by any other process with the seat token is ignored, and with no local record nothing is promoted or listed as older.
- The seat book and the claim index are replaced whole: the new bytes
  are written to a temp file in the same directory, synced, and renamed
  over the old name. A cut during that write leaves the previous file
  or the new one. A note queue, a ballot and a sync export already
  landed by rename.
- The timeline adds an island row only when the activate call carried this seat's token. A listener that answers anyone can put a sentence on the island, and the row would otherwise be that sentence, dated as a memory.
- The tracker history check followed an exported `GIT_DIR`, so a shallow tracker read as complete when that variable named a complete repository; history reads and the deepen fetch now drop the repository variables. Two checks of one tracker no longer fetch at once: the second waits for the first, then reads the checkout again. A deepen whose fetch failed now reads the checkout again before it reports incomplete history, and a checkout that is not shallow but still misses objects, as a partial clone does, names `git fetch --refetch --no-filter` instead of `git fetch --unshallow`.
- The user-rules file must be a regular file of at most 1 MiB. A FIFO, a directory, or a larger file is not read.
- Time a pack spends answering is not charged to a side event. One read waits at most 400 ms, so a slow answer still shows the memories.
- User rules written before a basis was kept stay in the preface, marked not confirmed, until `ljos prefer --user --review --keep` stamps them as the person's. A row written after that cutoff with no basis stays out. `ljos doctor` reports how many are held and still exits 0.
- Wget reads a `-e` or `--execute` command as a wgetrc line, with the
  name in any case and every `-` and `_` dropped, so `-e post-data=x`
  sends a POST. The guard matched only the underscore spellings, judged
  the request a GET, and let it reach GitHub or follow a redirect there.
  Every spelling of `post-data`, `post-file`, `body-data`, `body-file`,
  `method`, and `max-redirect` in `-e` is read now.
- When the user-rules file is present but cannot be read or parsed, GitHub writes those rules cover are denied and the reason names the file. Reads and other MCP calls stay allowed.
- Whether a writer takes `rank=relevance` is remembered per auth mode.
  An unauthenticated status no longer answers a later read that carries
  the token, and a refusal clears only the mode that was refused.
- `beforeMCPExecution` and `beforeReadFile` answer from the local user-rule file before they wait for a hook slot. The direct path has already loaded the seat env file. A promoted rule may still ask the pack once, for up to 400 ms. An allow or a user-rule deny exits 0. Exit 2 is a hook failure. A slow server leaves that allow on exit 0, and the call answers without searching the pack for a sitting.
- `beforeMCPExecution` and `beforeReadFile` are written to `hook-trace.jsonl` when the trace marker is present.
- `curl -L` and plain `wget` follow a redirect, and a 307 or 308 keeps
  the method, so a POST to another host could end at GitHub. Curl's
  `-L`, `--location`, and `--location-trusted`, and wget unless
  `max_redirect` is 0, now leave the target of a write unknown.
- `curl -g --no-globoff` turns URL globbing back on, and the guard
  still read the URL as literal. The later option now wins.
- `install.sh` checks the Rust toolchain before any source build and names the floor, 1.89 or the tree's `rust-version`, with a pointer to rustup. A distro cargo older than that failed on the first crate without saying what it needed.
- `ljos findings --remember` or `--issue` and `ljos panel --jev` check the seat before they write the tracker, as every other tracker write does. They ran with no checked seat, so the citation and the note were recorded under whatever seat the tracker derived.
- `ljos hook` and `prefer --user` believe a runtime's script only when the runtime is an installed `node`, `nodejs`, `bun` or `deno` of exactly that name, kept as a runner file is and the file the process runs; a program in `/tmp` named `node`, or one whose name only starts with `node`, no longer makes the script its arguments name the runner.
- `ljos hook` and `prefer --user` read the person's primary group from `/etc/group` itself, and only when `nsswitch.conf` sends group to that file alone; a second line with the same gid, another source, or a line that cannot be read means the group is not the person's alone.
- `ljos hook` and `prefer --user` read who else has the person's primary group from `/etc/passwd` itself, and only when `nsswitch.conf` sends passwd to that file alone; a source such as `systemd`, `sss` or `ldap`, or a line that cannot be read, now means the group is not the person's alone, where a short enumeration used to read as no one else.
- `ljos hook` and `prefer --user` refuse a process whose executable cannot be stat'ed through `/proc/<pid>/exe`, where they used to judge the path that link names without checking it is the file the process runs.
- `ljos hook` and `prefer --user` resolve a runner file's path once and judge the file their descriptor walk ends on only when it is the file the caller expects by device and inode, so a directory swapped for another after the path resolved is refused rather than judged in its place.
- `ljos hook` now waits for one of a fixed number of slots from `ljos hookd`, one server per user on a unix socket, before it loads the seat, the tracker or the pack. A burst of tool calls queues in the order it came instead of starting a full hook process each. A pre-tool call that gets no slot within 3 s, or finds 64 calls already waiting, is refused with the reason, and the wait counts toward the runner's deadline. `LJOS_HOOK_SLOTS`, `LJOS_HOOK_QUEUE` and `LJOS_HOOK_ADMIT_MS` change the defaults, and `LJOS_HOOKD=off` skips the queue.
- `ljos-mcp` holds the end of stdin until the longest configured call
  can finish. The wait used to be a fixed 35 s, so a pack call allowed
  more than that by `PACKSET_TIMEOUT_MS` was cut off and its answer
  dropped. The drain is now that timeout, a listing, the guard and a
  durable row lock, whichever is longest, plus a few seconds. The
  runtime's short shutdown cap stays in place for a dead stdin read, and
  grows to the same drain while a write is still in progress.
- `ljos-mcp` no longer drops a tool call when the client writes it and closes stdin at once: the call counts as in flight from the moment its line is read, so the end of input waits for its answer even when rmcp has not yet started the handler.
- `ljos-mcp` stops when the client that started it is gone, even if a
  shell, `npx`, or `sudo -n` is still sitting in front of it. The nearest
  ancestor that is not one of those launchers is recorded by pid and
  start time, so a reused pid is not taken for it. Stdin hanging up ends
  the server the same way. On Linux a failed `PR_SET_PDEATHSIG` is logged
  once; the signal itself is not the decision.
- `session_actor` and `stamped_sessions` now read `conversation_id` from the hook payload instead of consulting the process environment, and refuse when `conversation_id` or another runner's session key is present with an unusable value. For a `sand-sub` id, the parent's seat records and process match only through an ancestor whose own environment carries a usable `CURSOR_CONVERSATION_ID`, and `ljos sitting` on a ticket already held by the parent on the same seat files and claims a single child ticket under that parent through `file_issue`. Unbound calls read the conversation id from the outermost shell or caller within the current runner without rejecting one-shot `bash -c`, `/bin/sh -c`, or `zsh -c` exec chains, `clean_shell_environ` preserves `CURSOR_CONVERSATION_ID` in sorted `tmux` and `screen` pane environments and in `zsh`, `live_sitting` compares the conversation tag on the tracker, and `ljos_sitting` over MCP binds the calling conversation through `_meta`, per-call hook records, or the server's ancestor environment; an MCP client with no `_meta` id, no hook, and no stamped ancestor can no longer sit as the bare seat.
- `wget -i FILE` fetched every URL in the file and `wget --base=URL`
  resolved a relative one against GitHub, while the guard judged only
  the URL on the command line. An input file, a base, and a `Host:`
  header on the wget command line now leave the target unknown, split
  or attached, in a short cluster, or abbreviated, and the command line
  is read after a wgetrc the guard found opaque.
- curl, wget, and httpie send through a proxy named in `http_proxy`,
  `https_proxy`, or `all_proxy`, and the guard never read them, so a
  forwarding proxy could carry a POST to GitHub. A set proxy variable,
  from the process, an `export` on the line, or a prefix, now leaves
  the target of a write unknown. `NO_PROXY` does not exempt one.
- tokio 1.54.1, which clears RUSTSEC-2026-0386: the `AsyncFd` constructors took any `AsRawFd` without proof the descriptor was owned. ljos uses none of them, but the lockfile now resolves past the advisory and `cargo deny check` passes again.

### Documentation

- A missing user-rules file is no rules. An empty file, a whitespace file, and a dangling symlink refuse a covered GitHub write and name the path.
- Session end keeps up to five cue-word sentences from the person's and the agent's prose as unverified agent notes. The prompt hook does not inject them, and LJOS_EXTRACT=off turns the read off. The explanation, the getting started page and the integration table say so.
- The MCP and read answer may already have loaded the seat env file. A promoted user rule can still make one pack round trip of up to 400 ms.
- The README and the cloud VM page say that ljos cloud up and the cloud install script ship in v0.33.0.
- The README says that session end keeps up to five cue-word sentences as unverified agent notes the prompt hook does not inject.
- The README says the installer adds the bin directory to the shell profiles when it is not on PATH, and that this ships in v0.33.0.
- The README shows the install-to-recall recording under the title.
- The explanation says plainly that the home install roots and a single-user Nix store are trusted because the person owns them, and that a tool call running with the person's uid can write them too.
- The explanation says that finding the runner is Linux only: on macOS and other systems no process counts as an installed runner, so `ljos hook` keeps no prompt as the person's and `prefer --user` refuses.
- The pack proof cannot tell packsetd from a process of this user that can read the token file. That process can answer the two probes and then read the token, or skip the probes and answer as the pack. A writer that signs its answers, or that runs as another user, would tell them apart.
- `docs/cloud-vm.md` names `nc` beside `python3` urllib, dulwich, and
  `openssl s_client` as clients the shell guard allows because it does
  not read the request they send, and says a server-side ruleset is
  what stops one.
- `scripts/mcp-rss.py` refuses to report when `PACKSET_URL` is unset,
  empty, `off`, or not answering. A measurement against a dead pack is
  not a memory number.

### Miscellaneous

- A pack that accepts and never answers is covered with the queue off, for `beforeMCPExecution` and `beforeReadFile`.
- A runner variable refuses `ljos prefer --user --review --keep` when no terminal is attached. The test for that stands in that state, since a variable at a terminal is the person's shell.
- CI runs the mcp lifetime tests on `macos-latest`. That is the
  kqueue path for a client dying behind a launcher. The release workflow
  already builds on that runner.
- Seat, rules, and hook tests are named for the behaviour they cover.
- The curl command line is read into options and URLs by one function,
  so the guard's other checks can ask which words a curl reads as a
  config.
- The migration tests open a pty from Python and then keep a held lesson. Python is not a login session, so the keep now runs with the pty held by a terminal emulator, which is what a person at a terminal looks like.
- The release workflow pins CARGO_HOME to /usr/local/cargo and remaps the checkout to /src and that cargo home to /cargo.
- The remember and forget tests keep the POST when a seat token makes the writer prove first.
- The search-origin test now puts other words on the hit than the listing holds, and the open-listener test's fixture refuses some reads without a token while never checking the bearer, so each fails on the checks they were written for.
- The user-rule tests type a command as the person. A CI job sets `GITHUB_ACTIONS`, which marks the process as not the person, so the rule was stored as the agent's and the next seat did not see it. The simulated command does not carry that variable.
- `systemd-run --user --scope` answers on some hosts that then reject `--pty` on a scope. The transient-scope test probes the command it actually runs, and uses the `run-*.scope` cgroup directly when that probe fails.


## 0.33.0 (2026-10-22)

### Features

- A GitHub Actions run and a cloud agent take `LJOS_SEAT` plus the run id as the seat. That seat signs its vote and its handover with its own key, and a later process on the machine does not sign them again.
- A cloud seat's finish exports the deeds that seat cited on the issue into `.ljos/deeds/<seat>/`.
  The bag carries the file bytes, the inclusion receipts and any supersedes links, and the tracker commit takes it.
  A box finish writes no bag.
- A wake name that ends in `-` matches every longer seat that starts with it.
  Mail to a run then wakes the launcher named by that prefix, and the envelope still carries the run's seat.
  An exact wake wins, and the longest prefix wins when several match.
- An optional `[mail]` table (`notmuch_config`, `notmuch`, `identity`, `deny_file`, `attribution`) is read from the usual config layers and from `~/.config/ljos/mail.toml`. Without it nothing changes; a layer that does not parse or names an unknown key is an error.
- The release builds the Open Neural Network Exchange (ONNX) Runtime 1.28.3 from source with telemetry off and attaches it with `packset-embed` linked to it, each archive beside its SHA-256 sum.
  `ljos cloud up` installs the pair on a cloud VM only and installs neither when a sum does not match.
- The wake envelope names the tracker commit that holds the letter.
  A command receives it as `LJOS_WAKE_COMMIT`, and a webhook as `commit` in the JSON.
  The field is left out when push mode did not make a log commit.
- `LJOS_TRACKER_GIT=push` commits a tracker write, rebases with the per-issue field merge, and pushes. A failed push stays queued for the next write and the end of the session, and `ljos doctor` names unpushed commits.
- `ljos cloud up` installs the release binaries into `~/.local/bin`, checks their SHA-256 sums, writes the harness MCP command as a path that exists on this machine, and clones `LJOS_TRACKER_REMOTE`.
- `ljos cloud up` writes a webhook wake to `wake.toml` for each `LJOS_WAKE_<SEAT>_URL`, with `key_env = "LJOS_WAKE_<SEAT>_KEY"`, so a cloud VM wakes a box seat with its key passed by name and never written to disk.
- `ljos cloud up` writes the age identity for scope `cloud` from the environment variable named by `LJOS_CLOUD_AGE_SECRET` into `~/.config/ljos/cloud.age` with mode 0600, and `ljos doctor` reports that file.
  A virtual machine can read scope `cloud` with that identity, which stays out of the tracker.
- `ljos file --options` and `ljos options` write an `Options:` line a Jev ballot can read.
- `ljos finish --lesson`, `ljos findings --remember` and the correction hook file a proposal instead of writing the memory.
  An agent's lesson starts at L0, packset's `derived` level, with origin `agent-derived`.
  A campaign finding is tool output, origin `tool-echo`, and stays at L0 until `ljos accept` promotes the proposal to `explicit`.
  The correction hook files the person's words as origin `user-declared`.
- `ljos inbox --sync` fetches the tracker until the named commit is present, imports the sealed logs, and then lists.
  A commit that is not on the remote falls back to the remote head.
  With no remote the flag only lists.
- `ljos panel ISSUE --cloud` writes one prompt file per persona with the seat environment, the slot and the push rules, so a panel can start one cloud seat from each file.
- `ljos record check --tracker ROOT` checks every ballot on a vissue tracker for a CI gate. A ballot stands when its voter's newest `ballot-sig:` note names its choice, verifies, and is under the key `.ljos-seat-keys` pins for that voter and for no other seat; an unsigned ballot committed before signing began is legacy, as at the push gate.
  Each ballot that does not stand prints as `FILE:LINE: ISSUE: REASON`, and any one exits 1. The check reads the tracker and writes nothing to it, the pack or the network.
- `ljos record check DIR` now validates schema-v1 question and ballot entries,
  including canonical ballot ids, ed25519 signatures, run-key pinning, and the
  latest-per-voter tally rule where withdrawn ballots do not count.
- `ljos slot ISSUE SEAT --model MODEL --run RUN` signs a launch slot for a cloud run on the issue it is launched for, under the launching seat's pinned key. A seat the tracker first pins after slots began counts toward a push cite only under a slot on that issue from a seat pinned before then, and only when its own key was pinned after the slot. A pinned key alone no longer makes a voter.
- `ljos sync --adopt HOST` reposts that host's live atoms as this machine's, with origin `peer` and in that log's scope, then forgets the imported copies and drops the log.
  A log committed within the last 14 days is refused unless `--force`, and `--dry-run` prints the plan and writes nothing.
  Adoption stops without deleting the log when a post, a forget, or the fetch fails, and another machine retires its `sync:HOST` copies when it imports the adopt commit.
  A repository that names extra scopes adopts each of that host's logs in one commit, and the next import retires the `sync:HOST+SCOPE` copies of each extra scope whose log that commit removed.
- `ljos sync --adopt-legacy HOST` reads a host's logs from before the scope
  header, such as a gone box's cloud log sealed to this machine's key. It
  pins the SHA-256 of each such blob filed under HOST now, in this
  machine's state, and import and `--adopt` read exactly those bytes as
  HOST's. A copy under another name and any other blob stay unread, and
  the pin lapses once HOST writes a headed log.
- `ljos sync` now supports tracker repositories that name more than one scope in
  `.ljos/sync.toml` with a `[scopes]` table, each with its own age recipients.
  The primary scope keeps the existing `name` and `recipients` keys, and each
  extra scope exports to its own host log file under `.ljos/atoms/`.

  Exports and imports now preserve scope boundaries per host log so a cloud scope
  can be shared without exposing atoms from box-only scopes. Existing
  single-scope tracker configs keep their current behavior unchanged.
- `ljos_cli::mailbox::checks::refusals` refuses a draft whose From is not the configured identity, whose routing headers or recipients name a denied address, domain or handle, that carries a denied term or pair, that carries attribution text, or that the checks cannot read. The deny list is read from `[mail] deny_file`; matching runs after NFKC, case folding and removing invisible characters. The checks only refuse: no result approves a send.
- `ljos_cli::mailbox::export` writes a draft as an `ljos-outbox v1` approval file (draft id, outbox hash, an expiry at most 24 h out, a random nonce, then the exact message bytes) and returns its sha256. It refuses control characters other than tab and newline, and the bidirectional overrides, anywhere in the message, and a message without its own `Message-ID` and `Date`.
- `ljos_cli::mailbox::notmuch` reads a notmuch database for search and show with a fixed argv and a cleared environment. Message text comes back between nonce-marked untrusted markers, HTML is reduced to its visible text with `display:none` text dropped and flagged, links are shown as text, and attachments are listed by name, type, size and sha256 only. CI installs notmuch for its tests.
- `ljos_mail_search` and `ljos_mail_show` read the mailbox the `[mail]` table names. They are not in the core set; add them with `LJOS_MCP_TOOLS=core,mail_search,mail_show`. A cloud seat, or a machine with no `[mail]` table, gets a refusal.

### Fixes

- - When `ljos onboard` heals a TOML or YAML harness config, it now rewrites
    only the harness's own `ljos` command entry. Other server entries and
    comments in the same file are left untouched.
- - `ljos cloud up` now writes the real install directory (including
    `LJOS_BIN_DIR` overrides) into both `.profile` and `.bashrc`.
  - In `.bashrc`, the PATH export is inserted before interactive-shell guards
    so non-interactive launches still see `ljos-mcp`.
- - `ljos doctor` now gives file-specific MCP advice: harness configs point to
    `ljos onboard --harness NAME`, while `~/.cursor/mcp.json` points to
    `ljos cloud up`.
  - MCP command checks no longer fail only because the current process has a
    short PATH: doctor also checks `$SHELL -lc 'command -v ljos-mcp'`, warns
    when only the login shell can resolve the name, warns on a mismatched
    `ljos-mcp` binary, respects `env.PATH`, and treats Codex `$HOME/...` paths
    as literal.
- - `ljos onboard` now keeps an absolute `.../ljos-mcp` command when that
    path exists on this machine, and rewrites it only when the path is
    missing.
- - `ljos onboard` now writes a bare `ljos-mcp` command only when that name
    resolves to the same server binary this `ljos` ships with. When PATH
    cannot find it (or finds another install), onboard writes the absolute
    server path and reports why.
- A Claude Code subagent's `ljos vote` binds the ballot to the parent session plus the subagent's `agent_id`, which the `PreToolUse` hook reads from the payload.
  Two subagents of one session both count, the parent and a subagent count as two, and one subagent still casts one ballot.
- A Claude Stop whose payload matches the previous one within ten seconds is still booked when the transcript has grown, so that turn's deeds and lessons are kept.
- A Codex subagent's `ljos vote` binds the ballot to the root session plus the subagent's thread: `CODEX_SESSION_ID`, then `CODEX_THREAD_ID` when the two differ.
- A GitHub host written with a capital scheme, a trailing dot,
  userinfo in front of the host, or no scheme after
  `--proto-default` was not recognised. The same request that
  the guard used to refuse then passed, including a DELETE and
  a receive-pack upload.

  The host is taken from the authority after the last `@`, with
  the scheme's case and one trailing dot ignored. A schemeless
  URL counts when curl's default protocol or httpie supplies it.
- A `git config include.path`, `--remove-section`, `sed -i`, `perl -i`,
  `dd`, or a redirect onto `.git/config` could point `core.hooksPath`
  somewhere else. Doctor still read one global key, so the row stayed
  ok and the next push skipped the hook.

  A write is refused when the path is a gitconfig, whichever tool
  names it. Doctor asks git which hooksPath the seat would run. A
  throwaway repository's own local hooksPath stays available.
- A backslash at the end of a line now joins the next line before the
  hook and `ljos_guard_check` hand the line to `ljos-policyd`, as the
  shell joins them, so `cd project && \` followed by `make clean-all` is
  judged in `project`. Single quotes, `$'...'`, comments and an escaped
  backslash keep the newline, as in the shell.
- A ballot cast with no conversation id is bound to its run: the GitHub Actions run, else the Cursor cloud agent (`CURSOR_AGENT_ID` or `CLOUD_AGENT_ID`).
  Claude Code's `CLAUDE_CODE_SESSION_ID` and opencode's `OPENCODE_SESSION_ID` name the session even when other runner ids are set.
  Two reviewers on one seat, each voting from its own run, both count, and one run still casts one ballot.
- A ballot is no longer bound to the `NAME-shell` holder a shell runner's env file shares across every conversation on a box: with no other session the seat is the session, and `ljos vote` and the tally call it unverified, so two seats on one box are two voices and a hooked subagent binds its own session.
- A claim Jev is sure of reaches the prompt only when it shares a word
  or an alias with the prompt, or names its subject. At a probability
  of 0.75 or more Jev's answer alone let in a claim that had nothing in
  common with what was asked. The check is still free of age.
- A cloud finish writes a deed bag that `deedar import` can take.
  Product bytes sit in `bytes/` under a 64-digit lowercase sha256 name.
  A supersedes or successor file that already names a sha256 digest is left as export wrote it, signature line included. The store's one-line id is written as `<id>` and `sha256:<digest>` instead of being copied over that file.
  A carried deed made from a deed this bag does not carry gets a tab-separated `sources` file when export wrote none.
  A symlink in the store is refused, and a bag already published stays in place when the new one cannot replace it.
- A cloud guard rule file written by an older ljos is still a cloud seat, so finish keeps exporting deeds, and those rules are read from the command's arguments. `ljos doctor` warns that the file is stale, and `ljos doctor --fix` replaces a known older file. A missing file stays information and is not created. A file that is not one of those versions is left in place, and `ljos doctor` warns to run `ljos cloud up`.
- A cloud seat's finish refuses a tracker whose `.ljos` or `.ljos/deeds` is
  a symlink, or whose `.ljos/deeds/<seat>` is not a directory, instead of
  publishing the deed bag wherever the link points.
- A cold first prompt waits up to a second for the encoder and then recalls the memory the prompt bears on.
  It still says the pack is starting when the encoder has not answered in that time.
- A commit that no after-hook named, one made from a script or chained before a step that fails, is cited when the session stops.
  Only the commits HEAD's reflog says the checkout wrote since the sitting began are cited, so a pull or a branch switch brings in no deed.
- A conflicted atom log aborted every real merge. git runs the atoms driver with the work tree as its cwd and passes `%A` as a relative `.merge_file_XXXX`, and that path never sits under the absolute checkout, so the driver refused it and catch-up and the user-rules pull stopped. A relative temp is resolved against the cwd, and it still has to land inside the work tree.
- A curlrc could set the method and the token, and a plain curl
  to GitHub was still allowed. The guard treated that file as
  opaque only when the command already carried a token.

  The default curlrc, `CURL_HOME`, the XDG curlrc, and `-K` are
  judged for the method and body they set, including a nested
  config. A file that cannot be read is a write. `-q` or
  `--disable` as the first argument leaves the default file unread.
- A curlrc line the guard could not read, a `variable` or `expand-`
  option, or a `-K -` config could add a URL, and the guard still
  took the host on the command line as the whole target. A POST to
  another host was allowed while the config could send it to
  GitHub. Those now leave the target unknown, and an unknown target
  is judged as GitHub.
- A default curlrc or wgetrc path that held a directory, a fifo, or
  a symlink counted as no config, so a curl or wget to GitHub was
  allowed. Only a missing path is no config now. Anything else that
  is not a regular file is a write, and a fifo does not hold the
  guard waiting for a writer.
- A depth-1 tracker checkout has no parent commits. Reading a missing
  parent as "the pin was not there" refused a seat that had been pinned
  before slots, and reading it as "no slots yet" treated a pin at HEAD
  as standing. When the checkout is shallow or missing objects, ljos
  fetches `--unshallow` from the tracker remote (and deepens if it is
  still shallow). If the history is still incomplete, the refusal says
  `tracker history incomplete` and names `git fetch --unshallow`.
- A failed command no longer drops the lesson that names it by chance.
  The failure note asked for two of the pack's ballots to name a hit.
  Where many lessons tie on the words the pack scores, which of them a
  ballot keeps at its cut is arbitrary, so the same old lesson came
  and went from run to run. The note already asks a lesson to name the
  failure's subject and share two of its words with it, and those now
  stand alone.
- A failed command no longer recalls lessons that only share the words
  every failure and every rule say. `build`, `failed`, `test`, `error`,
  `always` and `never` counted toward the two words a lesson has to
  share with the error, so a pool of such lessons all cleared the
  floor of a best that was just as weak. Those words no longer count,
  and a lesson's match on what failed has to reach a fixed minimum.
- A failed command recalls a lesson that names its tool by another name
  in the same family. The two words a lesson has to share with the
  failure were counted without aliases, so a lesson on `pnpm` and the
  registry shared one word with an `npm` registry error. Aliases now
  count, and a word in several alias groups still counts once.
- A failed command recalls a memory that shares its words however
  old that memory is. The failure note held a hit to 0.6 of the top
  fused score, and the pack decays that score by age, so a lesson
  that names the failure lost its place to a fresh hit. The floor
  is now 0.6 of the best lexical match, which carries no age, and
  the lessons it lets in are ordered by that match, not by the
  pack's fused score.
- A failed encoder install no longer leaves the new ONNX Runtime in place and drops the previous one.
  `ljos cloud up` stages the runtime and `packset-embed`, swaps them together, and puts the previous pair back if either rename fails.
- A fifo in place of a state log made `ljos doctor` wait in open until a writer appeared.
  State-log opens now use O_NONBLOCK, keep only a regular file, and the doctor reports that log as damaged.
- A fine-grained token that cannot read the check rollup falls back to workflow runs and the combined commit status.
  The gate message names the source it used, the rollup or the REST fallback.
  A completed run must conclude success, neutral, or skipped.
  A failed, cancelled, timed out, or action-required run is refused, and a queued or in-progress run is refused as pending.
  The combined commit status must be success when any status exists, and zero statuses is fine.
  When both are empty the merge asks, and the ask names a local evidence record when one exists for that sha.
  When those REST calls fail, the refusal still says gh could not read the pull request and does not say that CI is missing.
- A fresh clone offered `ljos sync --adopt-legacy` for a headerless log that replaced a host who had already committed a headed one. The note that the host writes headers lived under the tracker path, so the new checkout did not have it. Any committed headed blob of that log now counts, and adoption is not offered again.
- A gate refusal puts a full stop between the rule's reason and the detail after it.
  The warning a plain push prints with no passing test record keeps its own words.
- A global `core.hooksPath` the user already set is left alone.
  `ljos git-hooks install` appends a block that runs `ljos git-hook <name>` to each hook in that directory, creating a missing hook executable and keeping the existing text.
  `ljos doctor` passes when those hooks run ljos, and otherwise names `ljos git-hooks install`.
  `ljos git-hooks uninstall` removes only that block.
- A guard bypass log or note queue whose last row had no newline was read as healthy by `ljos doctor` for good, since that row was taken for one still being written.
  Both doctor rows now name the log damaged, with "an unfinished last row", once that row has gone unchanged for five seconds; a row a writer is still appending is left alone.
- A guard fault now tells the agent to ask the person to run
  `ljos doctor` in their own terminal, and names
  `LJOS_GUARD_FAIL_OPEN=1` as the switch that allows other lines.

  `ljos doctor`, `ljos doctor --fix` and `ljos onboard` still run
  while the guard is faulted, so the seat can be repaired. A line
  that also runs another command stays denied.
- A guard fault on a prompt is an allow with a note again. The relay
  was denying those events, and its five-second wait is shorter than
  the prompt hook's eight-second deadline, so a wedged memory pack
  blocked the person's prompt on Gemini, Qwen, Factory, OpenHands,
  goose and Cline.

  Fail-closed denies stay on tool calls. A prompt waits out the hook's
  own deadline.
- A harness MCP command that does not exist is a failing `ljos doctor` row, and the plugin says the MCP server has no tools.
- A held proposal used to be appended to `proposals.jsonl` with a plain open, so the file kept the default mode and a symlink in its place was written through. Each proposal is now one locked write, and the file is mode 0600.
- A here-document body is no longer read by `ljos-policyd` as commands
  unless a shell runs it. `cat <<'EOF'` over a line `sudo reboot` was
  refused, while `bash <<'EOF'` over `make clean-all` was judged where
  the line started. A body fed to a shell, directly, down a pipe or to a
  command that may run it, is now judged as the script it is, in the
  directory the line is in by then; the substitutions of an unquoted data
  body still are; a quoted data body is not judged at all. A `<<` inside
  `$((...))`, `${...}`, `$[...]` or `<<<` opens no here-document, and a
  delimiter the hook does not read leaves the body to be judged as
  commands.
- A hit the hooks leave out no longer sets the bar for the ones they
  let in. One hit with a word no other hit carries scored near 2 on
  the lexical match, the floor rose above 1, and every memory that
  named the subject was dropped. The floor is now taken over the hits
  that pass the other tests, and naming the subject is worth as much
  as a rare word.
- A hit the pack's cross-encoder did not score no longer freezes the
  order of a prompt's memories. When one hit the writer chose came back
  with a null score, ljos kept the writer's order as it stood, and a
  lesson the writer put last behind three scored lookalikes was cut by
  the cap of three. Hits with a score are now read by it, and one
  without goes in ahead of the first scored hit it matches the prompt
  better than.
- A hook and the MCP server share one live row for a seat and a conversation, so a later write, start, or note joins that issue instead of filing another.
  A commit whose command names an open issue is booked on it. A sitting that never claimed, and an MCP shutdown, leave no row without an issue. A stale claim is reaped when its runner is gone or the row is old. Releasing a claim puts a STARTED issue with nobody on it back to TODO.
- A hook holder whose old pid tag is all letters, such as `free-agy-aak` or `free-opencode-nrv`, again counts as its seat in the checkout's bookkeeping, unless the whole name is a seat the tracker knows (`eessi-bot` stays apart from `eessi`). A tag of digits alone still never folds.
- A hook's call record now names a session only for an `ljos` that runs under the process that ran the hook.
  The `PreToolUse` hook keeps its own ancestors with each shell call, and `ljos vote` takes a record only when the nearest of them still alive is one of its own ancestors and the line matches, so a record written by hand or left by a hook in another process tree binds nothing.
- A hook-filed issue takes its project from the tracker whose remote matches the checkout, or from the origin stored in the checkout, and a promptless title uses that same name.
  A clone folder is no longer the project when `git remote get-url` fails, follows `GIT_DIR`, or rewrites the URL.
- A hostile encoder archive can no longer install a symlink, a hard link, or a path that leaves the tree.
  `ljos cloud up` lists both archives before it unpacks either, and it keeps only regular files.
- A kept prompt is the person's only when it agrees with the runner's own record of the turn: the Claude Code transcript or Codex rollout the hook payload names, inside that runner's own directory, or the Cursor agent transcript of the conversation the runner's environment names. A script that rewrites the kept prompt and its bind without spelling their path is refused, and so is a kept prompt or bind whose ctime or mtime is later than the hook's time.
- A kept prompt now counts as the person's words only when its bind names the runner session of the process asking. A forged `latest-prompt` and `latest-prompt.bind` with any other session, or none, no longer make `ljos prefer` or `ljos remember` store a sentence as user-declared.
- A last part of digits alone stays part of a holder or a seat again: `node-01` and `node-02` are two holders, `qwen-72` is not `qwen`, and `inky-10` is its own seat. Only an old pid tag with both a letter and a digit, such as `inky-39u`, folds to its seat.
- A lesson or a note the pack would refuse for its length is stored as shorter copies inside packset's sentence, word and character limits, and the ticket still gets the note as it was written.
  The error from a refused POST /v1/atoms names the status and the pack's reason.
  A copy the pack refused is not left in the note queue, and a copy that could not be sent because the pack was down or answered 401 is queued and sent on the next note, finish, doctor or session start.
  The opencode plugin passes the session id and the process environment through to ljos hook, so the lesson is written for that session with the owner token.
- A lesson read by id now stands only on a pack that asks for this seat's token: the read goes only with the token, and only after the same read without it was refused.
  A listener on the pack URL that answers without the token, or a packsetd run with `PACKSET_AUTH=off`, no longer confirms a promoted rule or an island lesson as the person's.
  A process of the same user that reads the token file can still answer as the pack, and only a pack that signs its answers or runs as its own user would tell it apart.
- A lesson the pack's relevance cut kept on one of its two ballots now
  counts as the stored memory it is. When a cut comes back full, ljos
  adds the stored memories that best match, but it skipped one the cut
  already held, so a lesson kept on one ballot stayed short of the
  agreement the judged prompt path asks for, while the same lesson cut
  out came in whole.
- A lesson written again after an amend or a rebase closes the lesson it replaces, so the next session is not handed the replaced commit.
- A lock held on `bypass.jsonl` or another ljos state log used to stall the hook that wanted to append to it, and with it the command the guard had already let through. The lock is now tried for 50ms; past that the row is skipped, and a skipped bypass row is reported on stderr.
- A log sealed to another machine stays a skip when `PACKSET_URL=off`, including when `age-keygen -y` cannot name this machine's recipient. Import no longer replaces those skip lines with a pack error, and a keygen miss does not stop the later hosts.
- A long-lived reader (the MCP server, a hook daemon) no longer keeps stating a user rule after the pack deletes or retracts the lesson it was promoted from. The pack is asked for the lesson on every read, and nothing the process recorded earlier stands in for its answer. A search in such a process also names each hit by the origin the pack holds now, so a preference the pack downgraded from the person's no longer reads as theirs.
- A memory Jev says bears on the prompt reaches it however old the
  memory is. The judged path held it to 0.6 of the top fused score,
  and the pack decays that score by age, so a correct old memory
  lost its place to a fresh one. The floor no longer reads the clock.
  A claim Jev is sure of has to share a word or an alias with the
  prompt, or name its subject. One that Jev is less sure of has to
  name the subject and clear 0.6 of the best lexical match. Jev's
  probability orders this path.
- A memory that names the prompt's subject reaches the prompt however
  old it is. On a pack that sends the encoder's cosine, the prompt hook
  held such a memory to 0.6 of the top fused score, and the pack decays
  that score by age, so a correct old memory lost its place to fresh
  ones. The floor is now 0.6 of the best cosine, which carries no age.

  The 0.66 cosine line for a memory that shares no word with the prompt
  is unchanged.
- A memory that names the prompt's subject reaches the prompt on a
  pack that sends no cosine, however old it is. That fallback held
  the memory to 0.6 of the top fused score, and the pack decays that
  score by age, so a correct old memory lost its place to a fresh
  one. The floor is now 0.6 of the best lexical match, which carries
  no age. What gets in is ordered by how much of the prompt it
  covers, or by the cross-encoder when it ran, and not by the fused
  score.
- A memory verdict, a proposal and a queued note shared the 50ms lock skip meant for the command hook's logs, so a brush with the lock dropped a decision without a word. These durable writes now wait up to 30 seconds and report failure instead; a drop or a merge records its verdict before it changes the pack, so a pack that stops answering mid-review leaves the claim live and marked rather than gone with no verdict; and a lesson the queue could not take is no longer reported as queued.
- A memory, a search hit or a lesson the pack sends with no origin, a null one or an empty one is no longer read as the person's, whatever basis it carries.
  A search hit without an origin, as a milli document comes back, takes the origin the pack's listing holds now, and a hit the listing does not hold is no longer named by what an earlier read recorded.
- A merge whose head sha has no CI checks asks, and `ljos approve` records the yes.
  The ask names that sha, and names a local evidence record when one exists for it.
  Red or pending checks are still refused.
  When gh cannot read the pull request, the refusal still names `gh auth login` and does not say that CI is missing.
  A pull request with no Bugbot threads merges once its checks are green.
- A missing `ljos-policyd` still allows the line and says the guard did
  not start. `POLICYD_REQUIRED=1` refuses that line, including when
  `POLICYD_BIN` names a file that is not there. CI and a fresh install
  are not locked out unless that switch is set.
- A negation on the line before a quote, or before a colon, still governs it: "Do not\nalways paste the key" or "Do not do this:\nalways paste the key" no longer lets "always paste the key" pass as the person's words. A sentence ends at `.`, `!`, `?`, `;` or a blank line for this test; the whole sentence across the line break still matches.
- A pack writer that lists `rank=relevance` and answers one search with
  a 401, a 429 or a 400 about something else is no longer searched as an
  older writer from then on. ljos took any 4xx to a ranked search as the
  writer refusing the rank, and asked it for the fused order, which
  decays with age, for the rest of the process. Only a 400 whose body
  names `rank` refuses it now. Any other 4xx fails that one search: no
  fused search is tried, the stored memories that best match are all it
  returns, and the next search asks for the rank again.
- A pack writer that lists `rank=relevance` in its status and then
  refuses it is searched as an older writer. The hook used to read the
  refusal as a writer that is not answering, gave up when `PACKSET_URL`
  was set, and fell back to the fused search with no stored memories
  added, so an old lesson behind fresh lookalikes never reached the
  prompt. ljos now asks again without the rank, adds the stored
  memories that best match, and does not send the rank to that writer
  in that workspace until it reads the writer's status again.
- A pack writer that refuses the relevance rank in one workspace is still
  asked for it in the others, and asked again in that one a minute later.
  ljos kept one refusal per process, by the writer's address alone, so
  every workspace on that writer was searched in the fused order, which
  decays with age, until the process ended, even once the writer listed
  the rank again. It now keeps what a writer said per workspace and reads
  the writer's status again after a minute.
- A persona brief lists what bears on the persona's domains, however
  old. It took the first five hits of a ten-hit search in the pack's
  order, which decays with age, so an old lesson on the domain could
  be cut for a fresher one that did not name it. It now asks for four
  times as many and keeps the five that best match each domain.
- A persona brief no longer lists unverified working notes under what the seat knows, and it labels the tracker recall, which may include logbook notes, as unverified.
- A persona brief's "What you remembered yourself" block now leaves out the persona's working notes and labels its own lessons agent-derived, as the domain section already did.
- A plain `git push` is never refused for a missing or stale test record, and neither is a commit and a push on one line. With `test` set in `.ljos/gates.toml`, the push runs, and the hook and `ljos_guard_check` carry a warning that names `ljos evidence record`. A merge pushed to your own repository no longer waits on pull request CI or Cursor Bugbot threads; `gh pr merge` still does. A push onto a protected branch asks only when `owners` is set and the remote's owner is not in it. A gate file that does not parse is a warning on a push, not a refusal. The ljos repository's own gate file no longer sets `test`.
- A policyd that hangs, crashes, or prints something that is not a
  verdict now denies the tool call. Claude Code, Codex and Cursor call
  `ljos hook` directly, so that fault used to allow the command, and
  Cursor's answer was a bare allow with no note.

  `LJOS_GUARD_FAIL_OPEN=1` keeps the old allow with a note. A missing
  `ljos-policyd` still allows, and so does a deny that only says the
  seat was not ready.
- A preference migration stamps `source.basis` `person` only when the atom's server time is older than a cutoff receipt written once into the pack by the first person-side run, and a `via:prompt` tag alone does not qualify.
  Search, the island, the command gate and session start do not run that write, and a process under a runner does not either.
  An atom newer than the cutoff stays unstamped and is not listed as an older preference.
- A preference stored before a basis existed is stamped `source.basis` `person` on upgrade when an old person-path writer left a mark: the prompt hook's `via:prompt` tag, or a terminal `ljos prefer` whose harness, seat and session are the same word with no turn.
  A runner session, a turn, a session-end tag, or an origin other than `user-declared` stays unverified, and a basis already set is left as it is.
  The stamp runs once per pack. `ljos doctor` counts the preferences still waiting, session start names that count, and `ljos prefer --review` lists them.
  Keeping one is refused under a runner and from a pipe, so the person runs `ljos prefer --review --keep ID` from a terminal of their own.
- A preference the pack stored as agent-derived is no longer handed to a prompt as a standing rule, named in the session-start line, or shown before a command it names; recall labels it an unverified agent preference.
- A preference the pack will not raise in place is stored again with the clock it already had.
  `ljos prefer --review --keep` names the id of that copy.
  A lesson hash still reads the same claim, and the original is closed.
- A primary scope named `cloud` is refused.
  That name is reserved for the shared extra scope, so unscoped atoms and finish lessons stay on the box scope.
- A process that leaves its runner with setsid, a double fork, script or a detached tmux still counts as under that runner when its cgroup, or on macOS the responsible process, names one.
  A keep and a prefer from that process stay the runner's.
  Leaving the cgroup, or handing the process to launchd, still gets past the check. A cleared environment does not.
- A prompt hook the runner's own program spawned still saves the person's stated preference.
  A payload piped into `ljos hook` from a shell, including one that detached from the runner, is not saved.
- A prompt is the person's on Cursor only when Cursor's own transcript holds it. With no transcript the prefer is refused as unverified, and a stated choice is not saved.
  Grok, OpenCode, Gemini and Antigravity keep no record ljos reads, so a typed prompt there is unverified in the same way.
  Claude Code and Codex still keep a typed prompt, and a stated choice in that prompt is still saved.
- A prompt sent while the encoder is still loading ranks the stored
  memories as the other hooks do. It scored a rare word by raw BM25,
  which could pass 1, and a memory that named the prompt's subject at
  a flat 1, so one word repeated in a memory about something else
  outranked the lesson that covered the prompt. It now reads the
  graded match, where naming more of the prompt scores higher and a
  rare word alone stays below naming the subject.
- A prompt that says "always" no longer brings in every rule that says
  it. The word lost its `s` before the rule words were taken out, and
  `alway` is on no list, so "always run the formatter" named a rule
  about squashing commits, and Jev's sure path let such a rule in. Rule
  words now go before the plural is taken off.
- A push cite with `PACKSET_URL=off` counted a pinned, signed persona
  ballot as a seat, because the roster was taken to be empty. An empty
  answer from the pack means there are no personas. A pack that is off
  has not answered, so the cite is refused with the same
  roster-not-readable reason an unanswering pack already gives.
- A push of a merge commit does not read the pull request, so a different head sha does not refuse it.
- A push to any branch runs CI and the docs build.
  The pages deploy still runs only from main, and publish and release still run only from a version tag.
- A queued memory copy used to be appended with `writeln!`, which writes the JSON and the newline separately. Two copies landing together could share a line, so the next send dropped one. Each copy is now one locked write, and the file is mode 0600.
- A quote of the person's prompt is read in NFKC with the typographic apostrophes `’`, `ʼ` and `＇` as `'`, so "Don’t do this: always ..." keeps its negation and an excerpt that drops it is no longer taken as the person's words. A quote typed with a plain apostrophe still matches a prompt typed with a curly one.
- A rage decrypt miss stays a miss when the parent has `LANGUAGE=fr` or `LC_MESSAGES=fr`. The child sets those to C as well as `LC_ALL` and `LANG`, which is the order sys-locale checks, so import does not stop before later hosts.
- A recalled claim reads as the person's words only when the pack stored it as the person's; the `via:prompt` tag alone, which a direct post can carry, no longer makes an agent's lesson the person's words in the prompt hook, recall or a brief.
- A recalled claim the pack did not store as the person's, other than a lesson, now reads as an unverified agent note, and note provenance tags are compared after NFKC, with zero-width and format characters dropped and lookalike letters folded, so a Cyrillic or zero-width tag no longer passes a downgraded note off as a fact.
- A recalled note is now labelled by its true agent-derived origin rather than its forgeable provenance entities, a recalled lesson says whether it is the person's words or agent-derived, and `ljos recall` lists working notes apart from the facts under their own heading.
- A recast after a withdrawal now writes a fresh ballot signature even when
  the voter picks the same option, so the tally and consensus still count it.
- A record ballot cast after a same-hour tie now counts. Two ballots from one voter in one hour no longer drop every later ballot of that voter on the question: only a tie in the voter's latest hour is ambiguous.
- A refusal for a push to main, a delete, a tag, or a hooksPath
  write was labelled as a cursor/* push. The label now says the
  ref is outside the seat allowlist, or that the pre-push hook
  would not run.
- A refused ballot or settle now says that each persona brief goes to its own subagent or run, which casts `ljos vote --as NAME` itself, so a coordinator neither voices its panel from one session nor reports verdicts in prose.
- A refused remember names the pack's status and reason on the error line. A claim of three sentences says to split it into two remembers.
  A trailing citation, such as `Source: URL`, is stored on the atom's source and is not counted as a sentence.
  A remember that fails only the word limit, the readability grade or the adverb ratio is stored as written and the pack's warning is printed, as prefer already does. A third sentence is still split or refused, so one claim stays at most two sentences.
  Recall matches a phrase to its abbreviation and an abbreviation to its phrase, so a query for the JOSS conflict of interest rule ranks a claim that says COI.
- A relay that cannot read its stdin, or whose hook panics, now denies
  a tool call in that runner's own contract. Exit 1 with no output used
  to let the command run on nine of the twelve runners. Gemini, Copilot,
  Cline and Auggie get a deny object, because exit 2 alone allows the
  command there. A prompt error still exits 1.
- A release tag publishes the docs. The github-pages environment allows the main branch only, so the deploy runs as a workflow_run on the default branch and checks out the tagged commit. The tag's own run still builds and checks the site.
- A replayed record ballot no longer erases its voter. A byte-identical copy of a ballot file, or the same ballot with its key and signature hex upper-cased, counts as that one ballot, so it neither drops the voter as two ballots in one hour nor, as a copy of an older ballot, hides the voter's current one.
- A rewritten lesson keeps the title it was written under, instead of the prompt that arrived later.
  Switching a sitting row to a different issue clears that row's bookkeeping for the prior issue, including its deeds, failures, asks and lesson linkage, so a later rewrite does not reuse the old title or supersede the earlier issue's atom.
  A lesson queued while the pack is down still names the atom it closes.
- A rule promoted from a lesson the pack does not confirm as the person's still denies or asks in the shell, outbound-post and MCP guards, and its denial now says the rule is unverified and why instead of reading as the person's rule.
  A deny or an ask only takes a command away, so enforcing an unconfirmed one never widens what an agent may run, and a slow, down or forged pack cannot lift it.
- A rule written through MCP `ljos_rule`, or by `ljos rule` and `ljos prefer --deny`/`--ask` anywhere but a terminal the person is at, is always stored with basis `agent`. Before, a reason clause found in the person's latest prompt raised the whole rule, pattern and verdict included, to the person's, and the kept prompt it was read from is a file the agent's login can rewrite.
- A run seat pinned to its key in upper-case hex now counts under a slot
  that names the same key in lower case. The check that the run's pin is
  the key its slot names compared the hex text, where every other seat
  and slot key check compares key fingerprints.
- A run's ballot no longer counts under its slot when the same pin line
  was also committed on a branch whose issue had no slot and then merged
  in. Before, the merge hid that branch's pin, so the slot looked older
  than every pin it vouched for.
- A runner env file ljos wrote is refreshed the first time a new version runs.
  Lines the user added stay, and ljos says what it changed.
  An env file ljos did not write is left alone.
- A runner skill ljos wrote is refreshed the first time a new version runs, and `ljos doctor --fix` refreshes it too.
  The sha256 of the bytes ljos wrote is kept beside the skill, and a file that no longer matches that hash is left in place.
  `ljos doctor` warns about a stale skill ljos wrote and does not fail the command for it.
- A runner's `ljos remember`, `ljos prefer --user` and `--promote`, and the MCP words path now take a sentence as the person's only when it is a whole sentence or clause of their latest prompt that keeps the negation or condition the person put on it (`not`, `never`, `don't`, `no`, `without`, `avoid`, `unless`, `except`, `if`, `only`, `until`). "always paste the release key" is no longer accepted from "do not always paste the release key". The stored text may differ from the prompt only in plain punctuation, so a quote cannot add a backtick or another mark the person did not write.
- A search hit no longer names its own origin. The hook, a recall and a search take each hit's origin from the pack's listing, and a hit the listing does not hold, or a listing that does not answer, is not the person's.
  A listener that stamps `user-declared` on a search hit the listing holds as an agent's can no longer hand that preference to a prompt as the person's.
- A search that already holds the environment lock migrates legacy preferences by taking that lock before the migration gate, so two searches cannot each wait for the lock the other holds.
- A search, a listing and a by-id read now share one proof that the listener asks for this seat's token. The token goes out only after the same read without it was refused.
  A listener that answers without the token no longer supplies a person's memory. Its hits can still be shown as an agent's, with origin and basis removed.
- A seat counts toward a push cite without a launch slot only when its name
  is on the standing list compiled into ljos (`disk-saver`, `inky`, `ljosbot`).
  A git author date no longer makes a newly pinned seat look older than slots.
- A seat whose key the tracker pinned before its first slot stands again
  without a slot, so the box seats' ballots count toward a push cite.
  A ballot from a seat that does not stand and has no slot is left out of
  the cite and named; it no longer refuses the whole issue.
- A seat whose shell names no session, as on a shared box, no longer binds every subagent's ballot to the seat: `ljos vote` takes the session and the subagent the `PreToolUse` hook saw run that line, so each subagent counts once and the parent counts apart from them.
- A send of the note queue at session start moved the bytes it could not read to the dated `.damaged` file without a word, since the hook prints nothing of it. `ljos doctor` now makes the note queue row a `warn` while such a file sits beside the queue, naming it and how many bytes it holds, until the file is read and deleted.
- A session's signature on one issue no longer refuses a ballot on a sibling issue.
- A slot counts only when the issue's own entry already held that line in
  the commit that introduced the run seat's pin.
  The same line on another issue no longer backdates it, and the slot
  check does not read a git author or committer date.
- A slot now signs the tracker's root commits beside the issue id, so a
  slot copied to an issue with the same id on another tracker, where the
  launcher is pinned to the same key, no longer verifies there. A slot
  signed before this no longer verifies.
- A socket, a symlink, a directory or an unreadable file in place of the guard bypass log or the note queue made `ljos doctor` report "no command has run past the guard" or an empty queue.
  The state-log opens now return a typed refusal, and both doctor rows name any read failure other than a missing file as damaged, with the path.
- A state log whose last row had torn off without its newline would glue the next row onto it, so both failed to parse and a good write vanished. An append now caps a torn tail with a newline first, under the lock, so the new row lands on its own line. A reader counts the lines it could not parse, and `ljos doctor` reports a damaged guard bypass log rather than calling it healthy and empty.
- A stated choice split after a dangling negation is read as one sentence. "Do not" at the end of a line no longer lets the next line be saved as its opposite.
  A line that already ends, and a blank line, still start a new sentence.
- A sync import that reads the taken list shared by scope name and host now removes it once the tracker's own list is written.
  Left in place, it was read again whenever the tracker had no list of its own, such as after an adopt; a failed write keeps it for the next import.
- A tag release and a release run by hand build ONNX Runtime from source instead of restoring a cache from another ref.
  The encoder archive carries the library as a regular file, and the release sums record that library's sha256 after the archive hash.
- A terminal preference or lesson written by v0.17 through v0.29, which left no `source.seat`, is stamped `person` when harness and session are the same word.
  A lesson the upgrade holds back is counted, listed and kept the same way as a preference.
  A lesson stored as the agent's is left out of that list.
- A terminal, a multiplexer or init that ran a hook's shell no longer owns the hook's sitting: tmux, screen, zellij, herdr, systemd, init, sshd, login and the terminal emulators the seat walk already refuses, `gnome-terminal-server` among them.
  Each outlives every sitting under it, so a sitting whose own shell had exited was never finished as orphaned while that terminal stayed open.
  The walk from the hook stops below them, and when only shells sit there, the outermost of them owns the sitting.
  A known harness (claude, codex, cursor-agent, agy, opencode and the rest ljos knows by name) now owns the sitting over a nearer `uv`, `npx` or `node` that only started the hook and exited with it, which had closed the Write's claim on the next `ljos_start`.
  A harness that runs as plain `node` still owns its sitting when no known harness sits above it.
  The book keeps the owner's start time next to its pid, so a pid the kernel has handed to another process no longer keeps a finished runner's sitting open.
  The walk reads pid 1 too, so a harness that is a container's pid 1 owns its sitting; any other pid 1 is the init and owns none.
  The walk reads `/proc` and holds on Linux only. On macOS the hook's parent still owns the sitting, so there `ljos_start` after a Write still finishes the Write's claim and files a new one.
- A text that moved from one scope to another stays in the pack across the sync that moves it.
  Import retires every log before it posts any, and it reads the primary log before `<host>+<scope>` logs.
- A tracker `index.lock` more than a minute old, with no git process working in the checkout, is now named when a commit meets it: its path, its age, that a killed git left it, and the `rm` that clears it.
  Every later commit used to fail on it with git's `File exists` after six seconds of retries, and the hooks that commit in passing failed without a word.
- A tracker commit that finds HEAD already holding the file, because another seat's commit carried it, now reads `tracker git: nothing to commit` instead of `commit refused: On branch master`.
  A refused commit always says why: git's first line on stderr, else on stdout, else that git was killed by a signal or exited without a word, where the line used to end at `commit refused: `.
  The seat-key pin and sync's log commit go through the same staging, with the same backoff on a busy index.
- A tracker git call that runs past a hook's budget, as the user-rules sync at every session start does, is now terminated and given two seconds to clean up before it is killed.
  Killed outright in the middle of an add or a commit, git left an empty `.git/index.lock` that refused every later commit on the tracker until someone removed it by hand.
  The sync also waits for the tracker's commit lock only until its deadline, and its merges hold that lock, so a session start behind a seat's commit no longer spends its budget waiting and then cuts its own git off.
- A tracker git command ignores an inherited `GIT_DIR`. That variable names the repository in place of `-C`, so one test exporting it could make another test miss `issues.org`. A dotfiles checkout named on the command still uses its own `GIT_DIR`.
- A tracker write aimed at an unprepared path under `target/` now reports that the path is not a tracker checkout instead of writing lock or exclude state into the enclosing repository.
- A user rule promoted from a lesson is no longer stated when the pack answers without that lesson. A pack that was down is asked again on the next read instead of being taken as empty for the rest of the process.
- A user rule promoted from a lesson is now bound to the record it was promoted from: its text, its `ts` and `valid_from`, and the origin the pack stored it under.
  A record the pack later holds under the same id is another lesson and does not state the rule, whether the id was retracted and posted again, in other words or in the same ones, or an update with an empty or null origin set an agent's lesson back to the person's.
  A rule promoted before this release kept no such record and is not stated until it is promoted again.
- A user rule promoted from a lesson is stated as the person's only when this read got that lesson from the pack within 400 ms, under the same id, stored as the person's.
  A pack that is slow, down, holds the read open or answers with another atom now leaves the rule out of the prompt, in every fresh hook and in a long-lived reader alike, where it used to state the rule from the origin kept at promote time or from an earlier read.
  Such a read also clears the person an MCP server recorded for that lesson, so its island no longer labels the lesson the person's.
  Rows no longer keep a promote-time origin.
- A voter that already holds a ballot is told how to replace it: `ljos vote ISSUE --withdraw --as NAME`, then cast again.
  A ballot cast after its voter withdrew now counts, where before the session's first ballot stood and the voter could not vote again.
- A wgetrc could set the method and the token, and a plain wget
  to GitHub was still allowed. The guard treated that file as
  opaque only when the command already carried a token.

  The system wgetrc, `WGETRC`, `~/.wgetrc`, and `--config` are
  judged for the method and body they set. A file that cannot
  be read is a write. `--no-config` leaves those files unread.
- A wgetrc or `-e` command could set an input file, a base, a proxy,
  or a `Host:` header, and the guard still took the host on the
  command line as the target. A POST to another host was allowed
  while the file could send it to GitHub. Those commands now leave
  the target unknown, and an unknown target is judged as GitHub.
- A withdrawn ballot leaves the tally and no longer decides a push.
- Added an end-to-end harness hook test that runs `ljos hook` from one process
  directory while sending a different payload `cwd`, and verifies the guard
  judges in the payload directory.
- Agy commits are cited when the after-hook payload names no event and no command, and from the git hook when agy stamped a conversation id.
  Opencode commits made from its shell or a subprocess are cited by the git hook into that session.
  An amend or a rebase moves the deed and the lesson onto the new commit and drops the citation of the replaced sha.
  The git hook claims as the same SEAT@TAG holder `ljos claim` uses.
  The env file `ljos onboard` writes names the same git hooks directory as the install, and `ljos doctor` reports an env file that names a different one.
- An MCP ballot from a Claude Code subagent binds the parent session plus the subagent's `agent_id`.
  The plugin and `ljos onboard` add a second `PreToolUse` group, matcher `^mcp__.+__ljos_.+$`, whose hook notes which subagent made each seat MCP call, and the server reads it back by the call's `_meta` tool use id.
  On those calls the hook says nothing else; the server still judges them.
- An MCP ballot from a Codex subagent binds the root session plus the thread the call's `_meta` names, as its shell ballot does.
  An MCP call from opencode is bound to the session in `_meta` (`ai.opencode/sessionID`), which is a subagent's own.
- An agent's claim is always stored as agent-derived with an agent basis, so soft prose from `ljos prefer` under a runner is refused by the pack instead of being kept as the person's. Every reader takes a claim as the person's only when it carries the person basis that the person-path writers now stamp. A preference stored before this change carries no basis and no longer reads as the person's until it is preferred again.
- An agy server that `ljos onboard` gave `LJOS_SEAT=antigravity` now sits as the tracker's agy seat, such as `free-agy`, the seat its hooks sit as.
  It sat as `antigravity`, so on the willma-20261021g seat replay the server's finish of the claim the write hook took under `free-agy@TAG` was refused as not the assignee, and the issue stayed STARTED.
  A harness id the tracker does not list is that harness's one seat on the tracker, and `antigravity` now also finds a seat named after `agy`.
- An explicit `LJOS_SEAT_KEY` is used as given again. When neither `LJOS_SEAT` nor `VISSUE_AGENT` was set and a run or conversation id was in the environment, signing swapped it for a fresh key minted beside it, so the ballot was signed under a key its caller never saw.
- An extra scope with no recipients is skipped on export, and the other scopes are still sealed and committed.
  An empty primary recipient list is still an error.
- An http wake URL is accepted only when its host is localhost, an IPv4 loopback address, or ::1.
  `ljos cloud up` refuses `LJOS_WAKE_*_URL` names that slug to one seat, marks the wakes it writes with `source = "env"`, and leaves any other wake for that seat in place.
- An idle `ljos-mcp` is cheap again. It runs on two workers and a capped
  blocking pool instead of one worker per core, and two minutes after its
  last call it drops the pack listing it caches and hands freed memory
  back to the system. A server that grew to 90 MiB answering a burst of
  calls now settles near 18 MiB. `LJOS_MCP_IDLE_DROP`, or `[mcp]
  idle_drop` in `ljos.toml`, sets the window; `0` or `off` keeps
  everything.

  The server also stops when the process that started it dies, even with
  stdin still open, and on either ending it answers the calls it already
  read before it exits.
- An imported memory keeps its kind and its horizon, and no longer keeps `via:prompt` or a person basis. A peer cannot supply the person's words.
- An unscoped atom is no longer sealed into an extra scope when `default_scope` names that scope.
  Extra-scope logs take only atoms whose `scope:` entity names them, and a `default_scope` that names an extra scope is refused.
- An unsigned seat ballot counts as legacy only when its issue's entry
  already held it in the tracker commit that brought the first seat pin,
  by commit order. The gate read the committer date of the ballot's commit
  before, which any tracker writer sets, so a ballot cast after signing
  with a backdated commit counted unsigned. A tracker that never committed
  a pin has no legacy ballots; `LJOS_UNSIGNED_BALLOTS=allow` is for that.
- Asking git history whether a host had written headers parsed `.ljos/sync.toml` to pick the log file. A primary scope named `cloud` is reserved, so that parse failed and adopt kept the log. The primary name and the `host+scope` name are both walked, and a scope file that does not parse is not a reason to stop.
- CI and the cloud-finish import test build the deedar commit named in `.github/deedar-pin`.
  That commit binds each `bytes/<sha256>` file to its contents, so check refuses swapped product bytes.
- CI and the cloud-finish import test build the deedar commit named in `.github/deedar-pin`.
  That commit refuses a forged link and a deed folder named for another deed.
- Cloud guard now follows git's remapping for a bare refspec name
  (`git push origin branch`), including `remote.<name>.push` and
  `push.default=upstream`, before deciding whether the destination is under
  `cursor/*`.
- Cloud guard now reads `GIT_*` command-line environment and `env -C` chdir
  context when it checks a push, so repository, config, and branch reads
  match the checkout git will actually use.
- Cloud guard now refuses `git push --prune` refspec patterns that cover
  `refs/heads/*`, and refuses `git push --mirror`, because both forms can
  delete remote-only `cursor/*` branches.
- Cloud guard now refuses additional shell-trick push forms: unresolved git
  subcommand words, xargs-fed push arguments, `git send-pack`, `git subtree
  push`, and `git submodule foreach` scripts that would push to `cursor/*`.
  It also decodes ANSI-C `$'...'` escapes in shell words.
- Cloud guard now resolves git push aliases from configured
  `alias.<name>` entries (including `!git push` forms), from `--config-env`,
  and from earlier `git config alias.*` commands in the same line.
- Cloud guard now treats `git --attr-source` and
  `git push --recurse-submodules <mode>` as valued options while it parses a
  push command, so `cursor/*` destinations are still denied through those
  forms.
- Cloud guard now treats `heads/cursor/*` and `tags/cursor/*` as
  `cursor/*` destinations, matching git's short ref aliases for
  `refs/heads/*` and `refs/tags/*`.
- Concurrent fail-open hooks used to split one `bypass.jsonl` row across several writes, so the only record of a command the guard let through could no longer be read. Each row is now one locked write, and the file is mode 0600.
- Concurrent hooks used to split one `hook-trace.jsonl` row across several writes, so a live session's hook contract could no longer be read back. Each trace row is now one locked write, and the file is mode 0600.
- Concurrent judge answers used to split one `jev-log.jsonl` row across several writes, so a probability that should have been scored could no longer be read. Each answer is now one locked write, and the file is mode 0600.
- Concurrent memory reviews used to split one `memory-reviews.jsonl` row across two writes, so a keep, edit, or drop could no longer be read back. Each verdict is now one locked write, and the file is mode 0600.
- Develop and main finish the workflow run already in progress, and a newer push on those branches waits.
  Every other branch still cancels the run it replaces.
  The docs workflow uses the same cancel rule.
- Each seat and record voter now gets its own ballot key file: `a/b_c` and `a_b/c` used to share `seats/a_b_c/ballot.key` and sign with one key.
  Key directory names escape every byte other than ASCII letters, digits, `-` and `.` as `%HH`.
  A key at the old folded name, such as a judge's `judge_<model>`, moves to the new name on first use, so its tracker pin still matches.
- Every ljos write to a tracker checkout's index now holds one lock, `.ljos-commit.lock` at the top of the worktree, kept out of `git status` through `info/exclude`.
  A tracker commit holds it from the status read through the logbook stamp, the add, the commit and the seat-key pin; sync's commits, the push's rebase or merge, and the merge-attributes commit take it too.
  A seat's first key pin is written under it and by rename instead of being rewritten in place and staged with no lock, so parallel first ballots no longer lose a pin or meet on `index.lock`.
- Every surface reads who stands behind a memory from one stored origin. The basis and origin are read without case, so a peer import strips a `Person` basis just as the reader honours it. Note tags no longer turn the person's record into an unverified note. A peer's preference, lesson or note is labelled as a peer's instead of an agent's.
- File order no longer decides which key holds a seat in the record. `ljos record check` now fails when one seat signs under two unpinned keys, whichever file comes first, and the tally counts that seat under neither key on the question instead of keeping the first one it read.
- First-run runner refresh no longer keys only on the semantic version.
  Skill and env refresh stamps now key on the bytes this ljos would write, so
  same-version upgrades still refresh stale files ljos wrote when shipped
  content changes.
- Guard-fault handling now has an explicit escape hatch:
  `LJOS_GUARD_FAIL_OPEN=1` restores the legacy allow-with-note behavior.

  By default, guard faults fail closed in runner relays. The switch is
  parsed with the same truthy forms as `POLICYD_REQUIRED`.
- Harness MCP configs name `ljos-mcp` on PATH instead of an absolute path from the machine that wrote them, so a file synced onto another home still starts. `ljos doctor` reports a configured command that is not runnable here. `ljos cloud up` puts `~/.local/bin` on PATH before it writes that config.
- Held-issue recall on session start and prompts, and the recall `ljos_start` returns, now leave working notes out so an agent's own note cannot come back as a fact.
- Holder and seat matching no longer read a name with a dangling separator as the seat: `ljosbot@`, `ljosbot-` and `ljosbot.` are not `ljosbot`, and `@abc` is not `runner`.
- Holder and seat matching now use one folding rule across
  `holder_parts`, `same_holder`, `same_seat`, `claim_holder`,
  `seat_key` and `adopts_holder`.
  Legacy pid tags still fold, including all-digit tags such as `inky-10`,
  while `sand-subagent-*`, `run-*` and `judge-*` names stay whole.
  Seat matching now folds case consistently (`GPT-5` and `gpt-5` are one
  seat; `Ridley-CI-A-555001` and `ridley-ci-a-555001` are one seat).
- Hook directory resolution now reads Gemini's `tool_input.dir_path` and Qwen's
  `tool_input.directory` as workdir overrides, matching the runner payloads those
  tools emit.
- In hook-child mode, tracker pin commits now run their ignore check in the tracker checkout itself, so an exported parent `GIT_DIR` cannot suppress the seat-key pin commit.
- Keep a live claim with the conversation that holds it, including a row adopted from a hold record, when two conversations share a seat.
  A tool call before the client identifies itself does not record the seat as runner.
- Legacy ballot dating now resolves the specific ballot line inside the issue entry before reading git history, so another issue with the same voter and choice text no longer changes that issue's legacy status.
- Legacy holders claim as their seat: `inky-n9o` and `disk-saver-t4t` as `inky` and `disk-saver`, and `ljos-bot` as `ljosbot`.
  `grokbot-sand-sub` is not a seat, so a claim under that name is refused and nothing is written.
- Long blocking findings now use the same truncation and normalization when
  matching filed dissent children, so repeated reads do not file duplicates.
- MCP `remember` and `prefer` refuse a claim that is not in the person's latest prompt and point the agent to `ljos_note` for its own findings.
  Recall, session start and a persona brief now keep those notes out of the prompt fact block, and `ljos recall` lists them apart from the facts, so an agent's own note no longer returns as a fact.
- On a git tracker a name on the standing list (`disk-saver`, `inky`,
  `ljosbot`) no longer stands by itself. The first writer to sign as a
  listed name the tracker had not pinned when slots began now needs a slot
  like any other seat. The list still stands on a tracker that is not a git
  repository.
- On macOS, `ljos doctor` looks for Claude Desktop's bare `ljos-mcp` on launchd's PATH, `/usr/bin:/bin:/usr/sbin:/sbin`, which is what the app opened from the Dock or Finder searches.
  It used to look on its own PATH and a login shell's, and call a command ready that the app could not start.
- On macOS, `ljos onboard --harness claude-desktop` and `ljos doctor` use `~/Library/Application Support/Claude/claude_desktop_config.json`, the file Claude Desktop reads there, instead of the Linux path.
  A harness row can name its macOS file with `config_json_macos`.
- On macOS, `ljos onboard --harness claude-desktop` writes this install's absolute `ljos-mcp` path unless launchd's PATH, `/usr/bin:/bin:/usr/sbin:/sbin`, runs it.
  Claude Desktop opened from the Dock or Finder starts its servers with that PATH, so a bare name found only on the shell's PATH did not start.
  A harness row marks such an app with `launchd = true`.
- One preference the pack refuses no longer stops the migration. The cutoff receipt is written first, a permanent refusal is recorded on it as `source.refused`, and that atom is not tried again. A pack that did not answer is tried on the next person-side run.
  A v0.32.1 prefer with `CLAUDECODE=1` and no runner ancestor was stored with harness, seat and session all `person` and is still promoted. Those bytes are not proof the person typed them.
  An atom that arrives with no origin is not a legacy preference. The pack stamps `user-declared` when a post leaves origin out.
- Only the tracker's `seats` roster decides whether an MCP server's harness id, such as the `LJOS_SEAT=antigravity` that `ljos onboard` writes for agy, is a seat of its own or sits as that harness's seat on the roster.
  An old `:CLAIMED_BY: antigravity@...` line from before the seat was `free-agy` had kept the harness id, so the server's finish of the claim the agy hook took under `free-agy` was refused as not held by `antigravity`.
  A roster that lists the harness id still keeps it.
  A harness id now sits only as its fixed `free-<command>` seat: `antigravity` and `agy` as `free-agy`, `codex` as `free-codex`, `claude` as `free-claude`, and so on for each harness ljos knows by name.
  It no longer takes a roster's only seat that merely starts or ends with the id, so a server with `LJOS_SEAT=claude` no longer finished the claims of a `claude-reviewer` seat.
- Prefix wakes now fire only for canonical run-seat slugs and skip the bare
  prefix key itself.
  Webhook envelopes quote the `read` helper safely and include `via` when a
  prefix matched.
  Wake output now names the launcher seat and the matched prefix, and
  `wake --clear` explains when a seat is covered only by a prefix.
- Prompt recall no longer injects `note` memories as facts: hook recall and
  issue recall injected into prompts now leave notes out.
  `ljos recall` still lists notes, but labels them as unverified agent notes
  and includes provenance tags where known.
  The MCP refusal for non-person `remember`/`prefer` text is now neutral and
  no longer tells the model to resave that text through `ljos_note`.
- Prompt recall now recognizes a refused note that was downgraded to another kind by the note provenance it still carries, so the downgrade can no longer smuggle it back into prompt facts.
- Publishing a deed bag keeps the directory fd from the open of `.ljos/deeds`
  and stages, swaps, and publishes with mkdirat, openat, and renameat on that
  fd, then fsyncs the directory. A symlink swapped in for `.ljos` or
  `.ljos/deeds` after the check is not where the bag is written. Atom logs
  are classified and written the same way, and the atoms merge driver writes
  through that fd.
- Reading a state log under `$XDG_STATE_HOME/ljos` still followed a symlink the writer refuses, so a link swapped in for a log file, or for the `ljos` directory, handed back whatever it pointed at. Every read now goes through the same directory-descriptor open the writer uses, and a symlinked log or directory is refused rather than read through. A `$HOME` or XDG base you made a symlink yourself is still followed.
- Reading a state log while a hook appended to it could catch a row cut in the middle of a character, and the doctor then reported no bypass at all, `ljos jev` an empty judge log, and `ljos accept` no proposals. A line an older ljos tore also hid every memory verdict after it. The readers of `bypass.jsonl`, `jev-log.jsonl`, `memory-reviews.jsonl`, `proposals.jsonl` and `note-queue.jsonl` now leave a last line without a newline for the next read and skip a line that does not parse.
- Rebinding a sitting onto another issue moves the commit baseline to the current HEAD, so commits already made for the previous issue are not cited on the new one.
- Removing an extra scope retires atoms previously imported from it, and export deletes this machine's stale `<host>+<scope>` logs.
  Readers no longer keep `sync:<host>+<old>` atoms after the scope is gone from the tracker.
- Retirement state for a synced host is stored per tracker, not only per scope name and host.
  Two tracker checkouts that share a scope name no longer retire each other's atoms.
- Rewriting the note queue used a `tmp + rename` through the path, which followed a symlink and left the file mode 0644, and the queue lock was created the same way. Both now go through the `ljos` directory's descriptor: the lock file is created mode 0600 without following a link, and a drain writes a fresh temp created `O_EXCL` mode 0600, fsyncs it, renames it over the queue from the directory, and fsyncs the directory. A queue file swapped for a symlink is refused.
- Runner hook relay faults now fail closed. If the inner `ljos hook` cannot
  start, times out, is signaled, or exits non-zero without a decision, the
  relay returns a deny in each runner's native deny contract instead of
  allowing the command.

  The relay also treats an inner-hook "guard is unhealthy" note as a guard
  fault and denies with a reason that names the fault and points to
  `ljos doctor`.
- Running a hooked `ljos vote` line a second time, as a shell loop or a pasted line does, is now refused as already cast.
  The call the hook saw stays behind as taken for its ten minutes, so the second run binds the session the first run bound, instead of casting again as the seat or as the parent session.
- Scope names must be non-empty and use only `[a-z0-9-]`.
  A slash or a plus in a name is refused at load, because both are special in the atoms log path.
- Seat and slot signature checks now compare signer keys and pinned keys by canonical key fingerprint, so uppercase and lowercase hex encodings of the same key are treated as the same key.
- Sending the note queue rewrote it with only the rows that parse, so a line cut short and a last line without its newline were deleted. Those bytes now move, unchanged, to a dated `note-queue.jsonl.<date>.damaged` file beside the queue before the rewrite, `ljos note` and `ljos finish` name that file, and a queue holding nothing readable is emptied into it.
- Session-end extraction now keeps each sentence from an assistant reply as an unverified agent note rather than a lesson, so the agent's own prose no longer becomes a fact the hook shows on later prompts.
- Sitting and finish under `LJOS_TRACKER_GIT=commit` no longer reach the network for the pack log.
  Sitting used to fetch every tracker remote and then read and post the pack, and a pack that did not answer in time printed `3: timed out reading response`.
  Finish still commits the sealed log when it changes, and it says the log was not pushed, including when the log was unchanged and a tracker commit was waiting to be sent.
  `ljos sync` run by hand still pulls and pushes.
- Starting work on the issue a sitting already holds keeps whether that ticket was named or filed, and only a rebind to a different issue resets those flags.
- Sync and guard-shim tests that spawn git now hold the shared test environment lock, so parallel lib tests no longer race on PATH or other process-global environment values.
- Sync import no longer reads a tracker log whose file name does not give a host name and a scope name.
  A removed scope's log was read by any suffix, so `peer+...jsonl.age` named scope `..` and its taken list was written outside the tracker's own state directory; such a log is now skipped with a line naming the part that was refused.
- Sync import now treats the same words on two tickets as two memories, as the pack does.
  A peer's atom on another ticket was skipped as already held, and the copy of the ticket the peer dropped was never retired, because import and adopt keyed what they held and what they had taken by text alone.
  The taken list now records text and issue; a list written before this change is read once through the copies this machine holds, so nothing is posted again and what the peer dropped is still retired.
- Taking over a placeholder issue keeps the sitting's failures, asks, title and landed commits instead of clearing that row.
- The Codex MCP entry `ljos onboard` writes now forwards `CODEX_SESSION_ID` and `CODEX_THREAD_ID` beside `XDG_RUNTIME_DIR`, so the server sees the session the prompt hook binds the kept prompt to. A Codex entry written before is brought up to date on the next `ljos onboard`; variables it already forwards are kept.
- The MCP server answers `tools/list` when the home directory is empty and no tracker or seat is set.
  Each listed schema is a JSON Schema object, with the draft identifier and schemars-only formats left off, so a strict client can load the list.
  A tool call with no seat still returns that error.
- The MCP server now counts the session a call's `_meta` names, as Codex sends it under `x-runner-turn-metadata`, among the runner's sessions when it checks the bind beside the kept prompt. A Codex MCP server, whose environment named no session, refused the person's real words. With no runner above the server, nothing counts, as before.
- The MCP tool `ljos_guard_check` now accepts an optional `cwd` argument and
  judges commands in that directory, instead of always using the server process
  start directory.
- The `- Voted by SEAT` logbook line ljos adds before a tracker commit is now written under the issue file's vissue lock and through a temporary file and a rename.
  Rewritten in place, it could drop a ballot another seat wrote at the same moment, and a parallel seat's `git add` failed with `confused by unstable object source data`.
- The atoms merge driver reads this host's fresh log, and writes git's
  merge result, without following a symlink. A tracker that committed
  this host's log as a link to a file outside it had that file copied into
  the merge result and pushed. Resolving a rebase writes the log the same
  way, and the conflict-marker check reads `.ljos` files without following
  a link.
- The cloud finish test read `accession=` from whatever `deedar` was on PATH.
  A released deedar prints `id=` and no such line, so the test panicked on a machine that had the release installed.
  The test now builds the deedar commit named in `.github/deedar-pin`, the same one CI installs.
- The cloud seat guard allows a ref write only when the destination is
  develop or a branch named in that seat's ljos.toml. A force, a delete, a
  tag, and any destination the line does not show are refused.
- The cloud seat guard no longer refuses a push whose destination an earlier assignment already names, `cd "$(git rev-parse --show-toplevel)"` (or the backtick form) before pushing `HEAD`, or `git push --tags` when no local tag is under `cursor/`. A destination the shell has not expanded is still refused, and the refusal names that whole destination. `git push --follow-tags` from a `cursor/` checkout is still refused, because that push sends the branch.
- The cloud seat guard now refuses `gh pr ready`, `gh pr reopen`, a gh alias `gh alias list` expands into one of those or into `gh pr create` or `gh pr merge`, and `gh api` calls that create, reopen, ready or merge a pull request. That is a write to a `.../pulls` or `.../pulls/N/merge` path, or a GraphQL mutation named `createPullRequest`, `reopenPullRequest`, `markPullRequestReadyForReview`, `mergePullRequest` or `enablePullRequestAutoMerge`. A GET of a pull request, a GraphQL query, an issue write, and `gh pr comment` stay allowed. A GraphQL body that is not on the line and cannot be read is refused, because it may be one of those mutations.
- The cloud seat guard that `ljos cloud up` installs now reads each `git push` and `gh` command's arguments instead of matching the command text. It refuses a push that names a `cursor/*` destination (`+cursor/x`, `refs/tags/cursor/x`, `HEAD:cursor/x`), a bare push or `HEAD` from a `cursor/` checkout, `--all`, `--branches` or `--mirror` when a local `cursor/` branch would be pushed, and `--tags` or `--mirror` when a local `cursor/` tag would be pushed. It also refuses `gh pr create`, `gh pr new` and `gh pr merge` with `-R` or `--repo` before the verb. Pushing a `cursor/` branch onto another branch, a remote URL or option value that contains `cursor/`, and gh comment text that mentions `pr merge` are no longer refused.
- The cloud-finish import test and CI build the same deedar commit, the one named in `.github/deedar-pin`.
  The test was still pinned to an older commit than the branch CI clones, so the import check failed.
- The cloud-up test clears an ambient GitHub Actions run id, so the seat suffix is the cursor agent id it sets.
- The destination allowlist now runs in the pre-push hook `ljos cloud up`
  installs. Git runs that hook for every push, including one a script,
  make, or another program starts, and passes the remote URL after
  insteadOf. The shell guard refuses the switches and the writes that
  would skip the hook, and `ljos doctor` reports a hook that is missing
  or was replaced.
- The docs workflows grant pages write and the identity token on the jobs that publish, and the checkout does not keep credentials.
- The first import of a log that git already stored with a header left no local note that the host writes headers, and it left a pinned pre-header blob in place. History is how a fresh clone knows. The note is still the file this checkout writes when it opens a headed log.
- The first sync import after taken lists moved under each tracker now still retires what a peer dropped.
  A tracker with no taken list of its own reads the one every checkout shared by scope name and host, instead of starting from nothing.
- The git shim treats a force push as the hook does. A `--force-with-lease` push onto a branch other than `main`, `master`, `develop` or a protected branch runs. A bare push reads the current branch with `git rev-parse --abbrev-ref HEAD` and the upstream from `branch.<name>.merge`. Any other force push stops with a message that it is the person's to approve, in place of the refusal that told the agent to give up.
- The guard and `ljos rules` take a rule's person basis as the person's only from a pack that asks for the seat's token. From any other listener every rule's reason is labelled an unverified agent note, as preferences already were.
- The guard check test keeps the guard on, so a parallel test's temporary home cannot allow a refused commit.
- The guard labels the reason of a rule the pack does not record as the person's, an agent rule or one with no basis, as an unverified agent note. Before, a hook denial or ask stated any stored rule's reason as though the person had written it.
- The guard no longer switches itself off in a temporary working directory, so `cd /tmp && rm -rf ~/proj` is judged. Only `LJOS_OFF` or a `HOME` that is a temporary directory switches it off, in the hook and in the rm, git and find shims alike, and `LJOS_GUARD=on` keeps it on under a temporary `HOME`. The session start line says `command guard off` with the reason, as `ljos doctor` does.
- The hook now hands `ljos-policyd` the shell line itself, as `bash -c
  LINE`, so it reads the line with its own shell lexer. It used to get the
  line split on spaces, so `(cd project; make clean-all)`, `cd project &&
  echo $(make clean-all)`, `cd project;make clean-all` and a `cd` on one
  line and `make clean-all` on the next were judged where the line
  started instead of in `project`.
- The hook now sends the full shell line to `ljos-policyd` from one resolved
  start directory, instead of tracking `cd` and `pushd` segments itself. A
  `cwd` with a leading `~` now expands against HOME, and an unresolved start
  directory is refused (`cwd-unresolved`) instead of falling back to the
  process directory.
- The hook now takes any string argument of a file tool that resolves into the runtime ljos directory as the tool's target, whatever its key (`target`, `destination`, `filePath`, `path` beside `text`, a path inside a list of changes) and however deep, including arguments Cursor passes as a JSON string. The seat guard refuses such a call for every tool that does not only read; `Read`, `read_text_file`, `search_files` and the like still reach the directory.
- The hook sends each full shell line to `ljos-policyd` from the payload start
  directory, and `ljos-policyd` decides where commands on that line run.
- The hook starts `ljos-policyd` in the directory each checked command argv runs
  in: the payload cwd plus any workdir override, and for chained pipelines after
  `cd` or `pushd`, the entered directory for later segments.
  When that directory is missing or not a directory, `ljos-policyd` stays in the
  hook process directory.
- The hooks, `ljos recall` and a persona brief no longer lose an old
  memory at the pack's cut. The pack returned its first twenty, or
  twelve for a failed command, by a score that decays with age, so a
  pack with enough fresh lookalikes never sent the old lesson that
  answers. They now ask the pack to choose by relevance alone. A pack
  too old to do that, or whose relevance cut comes back full, is
  joined by the stored memories that best match, aliases included, so
  a relevant old memory always has a way in.
- The kept prompt is the person's only when it is a whole entry the person typed in the runner's record of the latest turn, with spacing and Cursor's `<user_query>` wrapper set aside. Before, any span of that entry agreed, so a tool call could cut the kept prompt down to the one clause it wanted and still pass the record check.
- The latest-prompt file is now a guarded path, and a prompt counts only when the hook's bind matches its bytes and mtime. A shell write to that file is denied, and a file written beside the hook does not make a sentence the person's words.
- The mail checks and reader now fail closed. A draft must be one
  text/plain UTF-8 part in 7bit or 8bit with no encoded words, tags or
  entities. The approval hash is the message bytes `sendmail -i -t` is
  given, and export refuses Bcc, an invalid Date or Message-ID, a
  mismatched reachbook-hash and any draft the checks refuse. `[mail]`
  is read only from the user's own config files, needs `deny_file`
  with `identity` and an explicit `notmuch_config`. notmuch runs have a
  deadline, attachment hashing a cap, and show leaves out spam, refuses
  symlinked or out-of-root message files, flags hidden HTML and
  withholds what it cannot follow. Any cloud marker refuses the tools.
- The message for a skipped guard bypass row echoed the whole command onto hook stderr, which several runners show as the deny reason, so a secret or an injected newline in the command leaked there. The message now names only the log and the error.
- The no-runner onboard test now simulates missing tools with an injected command lookup instead of mutating process PATH, so parallel lib tests cannot hide git from sync tests.
- The npm push check no longer depends on one machine's node path. A CI
  job has no npm, so the test uses the installed npm when there is one
  and otherwise a script that runs the package.json recipe.
- The pack will not raise `source.basis` on an update, so an old preference cannot be stamped `person` in place. A person-side run closes that atom and writes a new one born with basis `person`. The closed atom keeps the basis it had. The cutoff notes the write before the close, and the next run finishes it. A runner still does neither.
- The persona brief keeps its heading saying the tracker recall may carry unverified logbook notes above the fenced tracker text.
- The pre-push hook exited successfully when LJOS_HOOK_KEY was set,
  before it read the allowlist. That variable was only there so a test
  could call the URL normaliser, and it shipped in the installed hook.
  The normaliser test builds its own script from the functions above
  the gate. The installed hook has no off switch.
- The pre-push hook took its allowlist from HOME or
  XDG_CONFIG_HOME, and it ran readlink and tr from PATH. A push
  could point those at another allowlist, or at programs that
  hid the real path, and the hook then allowed the push.

  The hook now resolves the allowlist from its own path, takes
  the tilde root from the passwd entry, and calls those programs
  by absolute path with PATH fixed to /usr/bin:/bin.
- The prompt `ljos panel ISSUE --cloud` writes no longer says the ballot counts only under its slot; it says the slot line is recorded for the launcher to sign.
  `ljos panel --cloud` refuses `--designers`, which it used to take and ignore.
  `ljos panel --cloud` refuses two personas whose names make the same seat, naming both, before it writes any file.
- The prompt `ljos panel ISSUE --cloud` writes tells the seat to load `~/.config/ljos/env` and cast its ballot `--as "$LJOS_SEAT"`, so the seat `cloud up` wrote signs it and the launcher's slot names it. It used to say to cast `--as` the persona and that the seat signed that ballot; the persona's own key signed it, and the push gate left it out before reading any slot.
- The prompt and failure hooks no longer lose an old memory before
  they test it. The pack cuts its list by a score that decays with
  age, and the hooks read only its first ten, in that order. They now
  ask for a wider list, and what passes is read by how well it
  matches, with the pack's score only breaking a tie.
- The prompt hook drops the kept prompt on every prompt before it keeps the new one, so a prompt it skips, such as a pasted block, or does not take as the person's no longer leaves the previous turn's words in place for up to an hour to be quoted as the person's.
- The prompt hook hands over the memory that covers the prompt, not the
  freshest of the ones that share a word with it. Every memory that
  named the prompt's subject scored the same, so on a pack with no
  encoder one rare word and then the age-decayed fused score picked
  among them: "the registry rejected my token" got three fresh
  lookalikes and never the January lesson on the expired CI secret.
  The match is now graded by how much of the prompt a memory covers,
  and when the cross-encoder ran, its order stands. Age only breaks an
  exact tie.
- The prompt hook no longer drops the lesson that covers the prompt
  because one of the pack's ballots left it out. Where many lessons tie
  on the words the pack scores, which of them a ballot keeps at its cut
  is arbitrary, so the January lesson on the expired CI secret came
  back on one ballot of two behind fresh lookalikes, and the prompt got
  three of those instead. A memory still has to name the prompt's
  subject and match it at least 0.6 as well as the best one, so a line
  that shares one rare word with the prompt stays out.
- The prompt hook now records every id the runner's payload names its conversation by (`conversation_id`, `session_id`, `sessionId`, `thread_id`) in the bind beside the kept prompt, and the check accepts the bind when any of them is a session the runner gave the checking process. Under Cursor, whose payload carries a `session_id` beside the `conversation_id` its shell is stamped with as `CURSOR_CONVERSATION_ID`, the person's real words were refused. Cursor's hook session is now its `conversation_id`.
- The push allowlist keyed a repository by dropping everything before
  the first `@`. A path that contained `@` then matched the seat, and
  the push was allowed. The key now keeps that path: userinfo is only
  the last `@` in the part before the first slash.
- The push gate now decides a cite on the ballots `ljos consensus` counts.
  A second ballot from the same session no longer lets the cite stand when
  consensus would refuse it.
- The push gate now reads `LJOS_CITE`: a push that carries a cite which
  does not stand is denied, with the reason the cite check gives. Before,
  nothing on the push path called the cite check, so a cited push ran
  whatever the issue's ballots said, and the claims that the push gate
  counts the same ballots as `ljos consensus` described no check.
- The push gate reads the tracker's seat pins strictly.
  A second line for a seat in `.ljos-seat-keys` no longer re-pins it to the
  new key; the gate refuses the pin file and names the seat.
- The push gate refuses a pin file that no longer holds a pin the tracker
  committed. Changing a seat's key in place no longer re-pins it, and
  removing a seat's pin no longer drops its ballot from the count.
- The ready line of `ljos doctor` names only the parts whose rows are `ok`: memory, tracker, command guard. Before, it said `memory, tracker and command guard work` while the optional tracker row failed, as it does when `vissue identity` exits 1 outside a tracker. The short doctor, and a sitting's short report, now print that tracker row under the line, with its fix. Other optional rows that are off, such as the encoder, stay behind `ljos doctor --all`.
- The record tally and seat pins keep a run's and a judge's id whole, as `record-v1.md` says. `judge-gpt-oss-120b` and `judge-gpt-oss-20b`, `run-pr-1234` and `run-pr-5678`, or `judge-grok-4` and `grok-4` are no longer one seat, and a seat row such as `inky` no longer holds `run-inky` or `judge-inky`.
- The relay no longer denies a tool call because its answer contains
  the unhealthy-guard sentence. That match missed the sentence once a
  push warning or a preference was joined to it. A real fault is
  already a deny from the check. An allow that carries the sentence
  stays that allow.
- The runner relay now treats Windsurf, Factory, Qwen and Crush as
  exit-code gates: a deny exits 2 and writes the reason to stderr, so
  their command guards block refused commands reliably.
- The same sentence noted on another ticket is stored as its own memory, and `ljos note` prints that memory's id.
  A pack that answers with the memory already held for a different ticket is given the note again under this ticket's set, which keeps the two apart.
  `ljos recall` on each ticket finds the note that was written on it.
- The seat guard no longer refuses a command for naming `latest-prompt` or `latest-prompt.bind` by itself unless the command writes the files it is given: a redirection, `mv`, `rm`, `ln`, `cp`, `install`, `rsync`, `tee`, `dd`, `touch`, `sed -i`, an editor, a shell or interpreter running a script, or a `find` that deletes or runs a command. `git grep latest-prompt`, `git log -S latest-prompt`, `echo latest-prompt`, `find . -name latest-prompt`, `test -f latest-prompt` and `cargo test latest-prompt` are allowed. A path that resolves into the runtime ljos directory is still refused to any command but a reader.
- The seat guard now also denies a tool call that renames, removes or links the runtime ljos directory, writes `latest-prompt` or its bind by a relative name or a glob, or writes one through an MCP file tool that names its target `path`.
- The seat guard now denies a tool call that assigns or exports a runner session variable (`LJOS_HARNESS_SESSION`, `CURSOR_*SESSION*`, or any `*_SESSION_ID`, `*_THREAD_ID` or `*_CONVERSATION_ID`) before it runs `ljos` or `ljos-mcp`, as a prefix, through `env`, `export` or `declare`, on an earlier line or inside `sh -c`.
- The seat guard now judges a write to the kept prompt by the path a word resolves to, not by how it is spelled. Variables with defaults, `$UID` and `~` are expanded, `.`, `..` and repeated slashes are taken out, a relative word is read from the directory the line has `cd`'d to, and the symlinks of the part that exists are followed, before the word is compared with the runtime ljos directory. A glob that could match the directory or a file in it counts, and so does moving, removing or linking a directory above it.
- The seat guard now refuses `ljos prefer`, `ljos remember` and `ljos accept` run through `ssh`, including `ssh localhost`. ljos started that way is not under the runner, so it took the words as the person's and skipped the check against the latest prompt; `ssh localhost ljos prefer --user --promote` could promote a lesson the person never said.
- The seat guard now treats interpreter code as a write to the kept prompt when the code names it. Inline programs given to `python -c`, `perl -e`, `node -e`, `ruby -e` or `awk`, and scripts fed on standard input or by heredoc to an interpreter, a shell, `git apply` or `patch`, are refused when they mention the runtime ljos directory, its environment spelling, or the files kept there.
- The seat guard refused writes to Claude, Codex and agy's hook
  files, and let an agent rewrite Cursor's and the relay runners'.
  Those files are the seat's law too. A guarded agent cannot
  overwrite them. Reading one still runs.
- The session a kept prompt's bind must name now comes from the process the runner started, read from that process's environment on Linux and macOS, and no longer from the asking `ljos`'s own. `LJOS_HARNESS_SESSION=forged ljos prefer --user ...` no longer picks the session of a forged bind, and with no runner above the process nothing counts as the person's words.
- The session an MCP call's `_meta` names no longer counts as the runner's when ljos checks whose prompt the kept one is. Only the environment the runner gave the process above, or the very runner process that spawned the prompt hook, which the bind beside the kept prompt now records, says a prompt is this conversation's. A `_meta` session only narrows the check: one that differs from the runner's, or from the bind's when the runner gave none, is refused for the verbs that store the person's words. A Codex MCP server, whose environment names no session, still takes the person's real words, because Codex itself spawned both the hook and the server.
- The stale-memory test reads recall counts from its own state directory, so a parallel test's recall of the same atom cannot hide a never-recalled row.
- The state logs refused a symlink only at the file itself, so a symlink put in place of the `ljos` directory under `$XDG_STATE_HOME`, `$XDG_DATA_HOME` or the runtime directory took every row somewhere else. That directory is now opened without following a link and the log is opened from it. A `$HOME` or XDG base you made a symlink yourself is still followed.
- The user-rule preface no longer states a rule promoted from a lesson the pack stored as agent-derived; a promote records the lesson's stored origin so a seat whose pack does not hold the lesson decides the same way.
- Tracker recall, the brief and the timeline fence tracker text with a nonce that is fresh on each call, and quote every line inside it. A logbook note that writes the closer, or a rule after it, now stays inside the fence.
- Tracker writes (`note`, `claim`, `sitting`, `finish`, `release`, `complete`, `file`, `options`, `deed --add/--remove`, and the matching MCP tools) refuse a seat nobody named, as ballots already did: a seat taken only from the program above the shell, or from the login user under a runner, is refused with a line naming `LJOS_SEAT` and the seats file. When `seats.toml` lists seats, a write from a seat it does not list is refused; with no seats file, writes go on as before. `ljos doctor` gains a `seat source` row that warns, without failing, when the seat is derived.
- User-rule sync refuses a symlinked `.ljos/user-rules` in the tracker or
  `sync/user-rules` in the state directory. Rules are no longer merged
  from, or sealed into, the directory a link points at.
- Wake writes now preserve an existing `wake.toml` mode on atomic rewrites and
  create new `wake.toml` files as mode 0600, so wake key variable names stay
  private by default.
- When VISSUE_ROOT is not a git checkout, seat signature refusals now include a one-time note that pre-signing ballots cannot be dated and are therefore reported as unsigned.
- When an agy hook payload omits top-level `cwd`, the guard now reads
  `toolCall.args.Cwd` before falling back to `workspacePaths`, so checks run in
  the tool call directory that agy reported.
- When nothing but the seat names a ballot's session, `ljos vote` says the session is unverified and that every run of that seat is one session.
  The tally marks those ballots unverified.
- Windows curl also loads `.curlrc` or `_curlrc` from `USERPROFILE`,
  `APPDATA`, and `USERPROFILE\Application Data`, and the guard did
  not read those files. They are judged now in curl's order, the
  Windows variables wherever they are set.
- With the pack down, the push cite line `ljos consensus` and the MCP
  consensus tool print now says the roster is not readable and that the
  cite does not stand, the same answer the push gate gives. Before, they
  read the roster as empty and counted every persona ballot as a seat's.
  A seat whose pack is off on purpose (`PACKSET_URL=off`) has an empty
  roster at the push gate too, as it already had in `ljos consensus`.
- `cloud up` now retires env-owned wakes before writing the cloud age identity,
  so a bad `LJOS_CLOUD_AGE_SECRET` cannot leave a stale wake URL that still
  sees a rotated wake bearer key.
- `cloud up` now retires stale env-owned wakes when the matching
  `LJOS_WAKE_<SEAT>_URL` variable is gone, collides with another variable,
  or names a refused URL.
  Wake writes now refuse to overwrite a malformed `wake.toml`, so a parse
  error aborts the write and leaves the file untouched.
  Wake webhook posts now refuse redirects, and wake file updates now use an
  exclusive lock plus temp-file-and-rename writes to prevent torn reads and
  lost concurrent updates.
  Entries without `source` remain treated as hand-set wake entries. This is
  intentional for entries written by the unreleased 2a30af8 behavior.
- `curl -G` turned every body into a read, so `-G -T` and `-G -F`
  still uploaded and passed. A curlrc or wgetrc could set the
  method, and writing those files was allowed. `wget -e
  method=DELETE` was a read.

  An upload stays a write beside `-G`. A client config is a write
  when the command carries a token, because the file can hide
  the method. Writing the config file is refused.
- `curl -LO`, `-fsSLO`, `-JLO`, `-C - -O`, `-i`, and `-4` were
  refused, and so were `--write-out`, `--retry-all-errors`,
  `gh run download`, `gh label list`, `gh cache list`, and a
  GraphQL query. Those are reads this seat runs.

  A short or long flag that only names a read stays allowed. A
  GraphQL operation is a read when it is not a mutation.
- `curl -X PUT https://ghcr.io/...` was allowed. The host check
  only named github.com, and ghcr.io accepts the same token.

  ghcr.io, githubusercontent.com, and pkg.github.com are GitHub
  hosts. A write to one of them is refused like a write to
  api.github.com.
- `gh api` treated `-fbase=main`, `-Ftitle=@file`, and `-if title=x`
  as flags it did not know. No field was recorded, so the call
  looked like a GET and a merge, a ref create, or a mutation
  passed.

  A short flag with its value attached is the same field as
  `-f base=main`. Any field is a write unless the method is GET
  or HEAD, or the call is a GraphQL query.
- `git replace` and a grafts file rewrite commit parents without making
  the checkout shallow or dropping an object. Pin order followed that
  rewritten parent list, so a pin removed before slots could still
  stand. History reads that decide pin order ignore replace refs and
  grafts. A checkout that has `refs/replace` or a grafts file is refused,
  and the refusal names which one.
- `git send-pack` does not run the pre-push hook, and the shell
  guard only checked its ref names. `develop` on another
  repository was allowed, and the push landed there.

  A send-pack written on the line is checked against the
  repository allowlist. A name git cannot resolve is refused.
  An ordinary `git push` is still left to the hook.
- `install.sh` puts its bin directory on PATH for the shells opened after it. When that directory is not on PATH, it adds `export PATH="$HOME/.local/bin:$PATH"` to `~/.profile` and `~/.bashrc`, and to `~/.zshrc` when there is one, under a marker comment that keeps a later install from adding it again. `--no-modify-path` leaves those files alone. Before, it printed a note and wrote nothing, so the next command after the install did not find `ljos`.
- `ljos claim` commits and pushes the issue the way a note does, so the claim reaches the other hosts before the next note.
  A `STARTED` claim stays taken when this machine's hold for that issue names a program that is still running, and goes back to `TODO` once that program is gone and the lease has passed.
  The hold names the nearest program above the command, so a short-lived runner is not kept alive by the daemon above it.
- `ljos cloud up` inserts the PATH export before Ubuntu's multi-line `case $-` guard in `~/.bashrc`, so a non-interactive shell that sources that file still finds `ljos-mcp`.
- `ljos cloud up` inserts the PATH export before an interactive guard written as `if [ -z "$PS1" ]; then`, `return` and `fi` on separate lines.
  A non-interactive shell that sources `~/.bashrc` used to return before the export and not find `ljos-mcp`.
- `ljos cloud up` no longer changes a scope cloud identity reached
  through a parent symlink. The mode and the bytes stay where they are.
- `ljos cloud up` no longer treats a directory named `packset-embed` as an installed encoder.
  The install check requires a regular file with the executable bit.
- `ljos cloud up` now creates the temp file for `~/.config/ljos/cloud.age` new, with mode 0600 and without following a link, and refuses a symlink at `cloud.age`, so a file or link planted beside it never receives the identity.
  `ljos sync` tries the `cloud.age` identity only on files sealed to scope `cloud`, so a VM never opens another scope's memory with it.
- `ljos cloud up` now writes a machine-local cloud guard rule file that denies `git push` to `cursor/*` refs and blocks `gh pr create` and `gh pr merge`; `ljos doctor` reports whether that guard file is present.
- `ljos cloud up` puts `LJOS_BIN_DIR` first on PATH. A stale `ljos-mcp` earlier on PATH used to stay the one a bare command ran, while cloud up reported the new install. The MCP config names the absolute path of this install when that earlier copy would have won.
- `ljos cloud up` recorded whichever repository it was run in.
  A second run from another clone rewrote the allowlist, and a
  push of develop then landed there. Doctor still reported ok.

  The first repository stays recorded. A later `cloud up` leaves
  it in place, and the shell guard refuses `--repoint`.
- `ljos cloud up` replaced an existing `core.hooksPath`. The
  identity hook booked there stopped running, and the guard then
  refused putting it back. The same rule refused a local
  hooksPath on a throwaway repository, so the bookkeeping fixture
  could not run.

  When a hooks directory is already booked, `cloud up` chains a
  pre-push that runs the previous hook and then the seat hook.
  The booked path stays. A throwaway repository can still set
  its own.
- `ljos cloud up` tags this machine's memory and lessons `scope:cloud`.
  They are sealed into the cloud log, and a box machine's unscoped memory stays on the primary scope.
  `cloud up` does not set `default_scope`.
- `ljos cloud up` writes the absolute `ljos-mcp` path unless PATH already runs this install when it starts.
  It used to write the bare name whenever the install directory was missing from PATH, or listed with a trailing slash or through a symlink, so a client that did not read the edited profile ran a stale copy or none.
- `ljos doctor` counted a `note-queue.jsonl.*.damaged` name in the state dir by looking through it when it was a symlink, so a link to a file outside the state dir showed as bytes a send had set aside. A damaged name that is a link, dangling or not, is now left out of the note queue row without following it or opening its target.
- `ljos doctor` leaves `$HOME` and `~/` as written in an OpenCode command. OpenCode expands `{env:HOME}` only, so a `$HOME/...` path that will not run was reported ready, and a working `{env:HOME}/...` path was reported missing.
- `ljos doctor` listed the note queue's damaged files by path, so a state dir swapped for a symlink could show files from outside it; the listing now goes through the state dir descriptor and a linked dir lists nothing. Emptying a note queue that is a dangling symlink is now refused instead of reported as done. A tracker deepen whose fetch failed, or whose shallow or missing-object state git could not report, no longer counts as complete history, and a failed fetch is tried again on the next check.
- `ljos doctor` names `ljos sync --adopt-legacy HOST` once for each host
  whose log still has no scope header. A develop reader already refused
  those logs; the row is how a seat sees the command, and the seat still
  counts as up.
- `ljos doctor` now gives a concrete repair command for an unresponsive
  `ljos-policyd` in the `policy` row, not only for a missing binary.

  The policy health check still fails both cases, and tests now cover
  missing and unresponsive binaries with the expected fix guidance.
- `ljos doctor` opened each log through the import path, which records that a host writes headers, and it still named a scope that had been removed. Looking is not recording, and a scope that is no longer configured is left out.
- `ljos doctor` passed over a note-queue row cut short but ended with a newline, so a queue holding only such a row read "empty" and one holding a note besides read as whole. Such a row now makes the note queue a `no` row that says how many rows are not a note and that `ljos finish` or the next session start moves them to a dated `.damaged` file. The guard bypass row says "the log is damaged" beside the rows it could not read.
- `ljos doctor` printed `ljos sync --adopt-legacy HOST` for every headerless file, including a name with a shell metacharacter and a host `-h`, which clap reads as help. The hint now uses the same name, duplicate, and scope checks as import, and the host is bound with `--adopt-legacy=HOST`.
- `ljos doctor` printed a damaged guard bypass log as an `ok` row, so the short doctor left it out. A bypass log with an unfinished last row that has sat, a row that cannot be read, or a fifo, socket, directory or symlink in its place is now a `warn` row the short doctor shows. The exit stays 0: the guard still judges every command.
- `ljos doctor` read the whole guard bypass log and note queue into memory whatever their size, so a runaway or swapped-in file held it for as long as the read took. Doctor now reads at most 32 MiB of either, and a log past that is a damaged row that gives its size and says it was not read.
- `ljos doctor` reads `env.PATH` from the ljos server's own table. `NODE_PATH`, `LD_LIBRARY_PATH`, and another server's `PATH` no longer fail a working config. JSON `env.PATH` expands `${env:PATH}` and `$PATH`.
- `ljos doctor` reads an MCP entry's `env.PATH` with the client's own syntax: `${env:PATH}` for Cursor, `${PATH}` for Claude Code, and `${PATH}` or `${env:PATH}` in a hermes YAML entry.
  It used to take `$PATH` as expanded in Cursor's file, where Cursor passes it as text, and to miss `${PATH}` in Claude Code's and hermes' files.
- `ljos doctor` reads home variables in `~/.cursor/mcp.json` as Cursor does (`${env:HOME}`, `${userHome}`) and in `~/.claude.json` as Claude Code does (`${HOME}`, `${HOME:-default}`).
  It used to call `$HOME` or `~/` in either file ready, though neither client expands them.
- `ljos doctor` said "ljos is ready" and exited 0 over a damaged note queue, and over a damaged guard bypass log. A damaged note queue now fails doctor: the summary says "ljos is not ready: the note queue is damaged" and the exit is 1. A damaged bypass log keeps its `warn` and exit 0, and the summary says "ljos runs, but the guard bypass log is damaged". An empty or missing log of either is whole.
- `ljos doctor` says `ljos cloud up` on a cloud VM, a codespace, or CI, and `ljos onboard` in a desktop Cursor terminal. `CURSOR_AGENT=1` alone is that terminal. CI is the documented value: `GITHUB_ACTIONS=true`, `GITLAB_CI=true`, `CI=true`, `BUILDKITE=true`, `TF_BUILD=True`, `CIRCLECI=true`, or a set `JENKINS_URL`. `false` does not count. A CI job still does not download `packset-embed`.
- `ljos doctor` sent the note queue before it reported on it, and a send rewrites the queue with only the rows that parse, so a last row still missing its newline, one touched a moment before or dated ahead of the clock, was deleted. The send also waited on the queue lock with no end, so a lock another process held hung doctor. Doctor, and the MCP doctor tool, now only read and count the queue; the next note or finish that reaches the pack, or a session start, sends it.
- `ljos doctor` tells a desktop Cursor install to run `ljos onboard --harness cursor`. It says `ljos cloud up` only on a cloud VM, a codespace, or CI.
- `ljos doctor` took a state log whose last row lacks its newline for one still being written whenever the file was dated ahead of the clock, so a touched or clock-skewed log never showed as damaged. A last row dated more than 5 seconds ahead of the clock is now damaged at once, and the row says it is dated ahead of the clock.
- `ljos doctor` tries the note-queue lock once instead of waiting on it, and when another process holds it the note queue row is a `warn` that says the queue is busy and names the lock, so a stuck holder shows in the short doctor rather than hanging it.
- `ljos doctor` used to wait forever on a habitat that never
  exited, and a policyd whose `version` answered while `check`
  hung looked healthy. A capture now stops at three minutes. The
  policy row runs `check` as well, and a hang there is unresponsive.

  A `POLICYD_BIN` that names a missing file says so, instead of
  suggesting a cargo install. `LJOS_GUARD_FAIL_OPEN=1` is its own
  row, and an allow it chooses is written to the bypass log.
- `ljos doctor` warns about a stale runner env ljos wrote and does not fail the command for it.
  `ljos doctor --fix` refreshes that file.
- `ljos doctor` warns when an MCP config names an absolute `ljos-mcp` that exists but is not this install.
  A stale copy left in another directory used to pass as ready, though it was the binary the client started.
- `ljos hook` and `prefer --user` count a runner as installed only below a real install root: `~/.local/bin`, `~/.cargo/bin` or `~/.npm-global` of the passwd home rather than `$HOME`, a system `bin`, npm's global `node_modules`, an app directory of the runner's name under `~/.local/share` or `/usr/share`, or a `/nix/store` entry.
  The path is compared after every link resolves, so a copy in `/tmp/.local/bin`, a project's `node_modules/.bin`, a `.local` a tool call made elsewhere, a link from `~/.local/bin` into `/tmp`, or a file another user could rewrite is not the runner.
- `ljos hook` and `prefer --user` trust a group-writable runner file only when the person's primary group bears their name and holds no one else, count a `/nix/store` entry only when Nix alone can change it, judge the directory each install root sits in, and read every directory down to the file through descriptors that do not follow a link, so the file judged is the one the process runs.
- `ljos hook` keeps a prompt as the person's only when the parent is an installed runner, older than the tool call the guard recorded, or that same runner. A copy named like a runner, and one started after the tool call in its cgroup or session, keeps nothing after `setsid`, `env -i`, `nohup`, a double fork or a reparent. `prefer --user` under that copy keeps nothing either. An unreadable session, cgroup or record keeps nothing.
- `ljos hook` keeps a prompt as the person's, and grants, saves or corrects from it, only when the runner itself spawned the hook: its parent must be Claude Code, Codex, Cursor or another runner as read from the file the kernel runs it from, with nothing between them but the `sh -c` that runs the configured hook command, and that runner must not run inside another runner's tool call. Otherwise the hook says why on stderr and keeps nothing. The seat guard now refuses a tool call that runs `ljos hook`, directly, through the plugin launcher or over ssh, since it writes the person's words.
- `ljos hook` now denies a shell command whose directory does not resolve
  as `cwd-unresolved`: a workdir or session `cwd` that is missing, a
  symlink loop, or a directory it cannot enter. It used to judge the
  command in the session directory, or its own, where the command does
  not run, and allow what it would allow there. `LJOS_GUARD_FAIL_OPEN=1`
  keeps that fallback, and a prompt is never denied for its directory.
  `ljos_guard_check` now refuses a `cwd` it cannot enter too.
- `ljos inbox --sync COMMIT` checks the commit against the fetched remote refs, so a commit made on this machine and never pushed is named as not on the remote and is not fast-forwarded to.
  It fetches again until the remote has the commit, for up to 30 seconds (`LJOS_INBOX_SYNC_WAIT`), so a push that lands just after the wake is still read.
- `ljos inbox` exits non-zero when a writer that reports no version refuses kind `message`, and the error names packset 0.13.0.
  An empty message is a tool dump on every packset and is refused before the kind, so that probe could not tell a pack that keeps mail from one that does not, and the inbox printed nothing unread.
  The probe is a message with a level the pack does not keep, so a refusal of the kind is an error and a refusal of the level means the pack keeps mail, and neither answer is stored.
  The reference and the howto describe the receipt `ljos hook --prompt` writes: at most eight messages, interrupts first, with the rest left unread, text lines `read`, `by`, `at` and `msg`, printed to the sender once.
  A shell writes that receipt before the hook exits. Cursor writes it when the note is delivered, on `postToolUse` or on `Stop` when the turn ran no tool.
- `ljos island` against a running pack labels an agent's claims by their stored origin again; the island row carries no origin of its own, and it had been read as the person's.
- `ljos memories review --edit` and `--merge` are refused under a runner.
  The replacement keeps the origin and the basis the memory already had, so a user-declared preference with no basis is not stamped `person` by the edit.
  An edit of a memory that already carries a basis keeps that basis.
- `ljos onboard --harness goose` rewrites only the `ljos:` extension's own `cmd`.
  A sibling extension listed after an `ljos:` entry with no `cmd` used to have its `ljos-mcp` path replaced, because the YAML entry was read as the next 24 lines.
- `ljos onboard --harness hermes` rewrites only the `ljos` command. A sibling server such as `ljos-dev` on the line above the marker keeps its own command.
- `ljos onboard --harness json` says on stderr why the command is an absolute path. Stdout stays the JSON entry alone, so it still parses. A PATH that does not resolve `ljos-mcp` used to print the path with no reason.
- `ljos onboard --harness opencode` writes the server at `mcp.servers.ljos` with `codemode` false, so opencode lists `ljos_ljos_recall` and `ljos_ljos_note` on the native tool list. With Code Mode left on, those names were missing and note and recall failed on every model.
- `ljos onboard` replaces a bare `ljos-mcp` that PATH resolves to another binary, or to nothing, with this install's path.
  `ljos doctor` used to warn about it and advise onboard, which left the config alone, so doctor warned again.
- `ljos onboard` replaces an absolute `ljos-mcp` in a harness config that exists but is another binary, the same way it replaces one that is missing.
  A stale copy in another directory used to be kept, and the config was left byte for byte as it was.
- `ljos panel --cloud` refuses two personas that boot as one seat, not only two whose names slug alike: `inky` and `inky-n9o`, `inky` and `inky@n9o`, and `ljos-bot` and `ljosbot` each name one seat, with no run id or with a short one that strips back off. `eessi` and `eessi-bot` stay two seats.
- `ljos prefer --review` prints the date, the seat and the source of each held preference or lesson, so one written a minute ago is not mistaken for an old one.
  The date is the pack's server time, or `source.at` when that is all the writer stored, and the seat falls back to the harness and then to a `seat:` entity.
  `ljos doctor` still exits 0 while those items are waiting.
- `ljos prefer --user --promote` under a runner, and MCP `ljos_prefer` with `user` and `promote`, now refuse a lesson whose text is not in the person's latest prompt, the same check `prefer --user` makes of the words.
- `ljos prefer --user` under a runner, and MCP `ljos_prefer` with `user` set, now refuse text that is not in the person's latest prompt. A user rule reaches every prompt and every seat, so a planted sentence is not stored.
- `ljos prefer` under a runner now stores only words from the person's latest prompt as theirs. Any other sentence is agent-derived, so a planted preference is not user-declared.
- `ljos prefer`, `ljos remember` and the lesson `ljos finish` keeps are the person's only when the person is at the terminal, by the same check `ljos rule` now makes. From a pipe, or below a parent named like a session, a preference is stored `agent-derived` and a remember is an unreviewed note.
- `ljos recall` lists what bears on an issue, not only what is newest.
  Past twenty of an issue's own memories it kept the newest, and of
  the related search it kept the first five the pack ranked, by a
  score that decays with age. It now asks for four times as many
  related hits and keeps the ones that best match the issue's id and
  title, with recency only breaking a tie. The count of what it left
  out now says "more" rather than "older".
- `ljos record check` again holds a voter id pin row to its seat. Beside `inky <key1>`, a row such as `mallory/inky <key9>` no longer lets a second key vote as seat `inky`; rows that pin one seat must pin one key, however they are spelled.
- `ljos record check` and ballot signature checks now refuse small-order ed25519 keys, which used to verify any message.
  An entry signed with the identity point as its key no longer passes, whatever its stance.
- `ljos record check` and entry signing now refuse voter ids whose part after `/`
  is not already that seat's slug, so `alpha/Seat-a` cannot be counted beside
  `alpha/seat-a` under one pinned key.
- `ljos record check` now accepts only exact lowercase ballot stances:
  `accept`, `reject`, or `withdrawn`. Variant spellings with spaces, Unicode
  invisibles, or capitals are refused.
- `ljos record check` now applies seat pins by key fingerprint, so hex-case
  differences and small-order-part variants of the same key are treated as
  the same signer.
  An explicit voter-id pin row now takes precedence over a conflicting seat
  row, while seat rows still reject a second key on that seat.
  Seat rows now cover run/judge aliases of a seat (`run-inky`,
  `judge-inky`) and keep true run/model forms on their own rows.

  Tally is now per question and key-based: one signing key is one voter on
  that question. A seat that appears under a second key on the same
  question keeps the first key's ballot and refuses the later key on that
  seat. Same-hour ambiguity drops only that voter's ballots on that
  question, so unrelated questions and voters still count.
  `replaces` now follows the effective voter identity (signing key), not
  the raw voter-id string.
- `ljos record check` now counts each signing key once per question, whatever voter ids it signs under.
  The latest hour still wins, and two unreplaced ballots under one key in one hour are an error.
  Keys that differ only in hex case or in a small-order part count as one key, since one secret signs under all of them.
  A voter id now has one written form: the identity is its slug, and the seat drops a holder `@TAG` or an old pid suffix and maps an old seat name, so `alpha/inky-39u` is `alpha/inky` and `alpha/ljos-bot` is `alpha/ljosbot`.
  A run and a judge keep their ids whole.
  Signing and checking refuse any other spelling and name the form to sign under; a stored entry in another spelling has to be signed again.
  Bare pin rows such as `inky@39u`, `inky-n9o`, `ljos-bot` and `judge:model-1.5` now hold record voters of the seat they read as.
- `ljos record check` now cross-checks `<identity>/<seat>` voters against any
  existing `<seat> <key>` tracker pin, so record entries cannot diverge from
  the seat key already pinned by `ljos vote`.
- `ljos record check` now fails closed on the tracker pin file: unreadable pin
  files and duplicate voter rows are hard errors, and pin mismatches report
  whether the source was the pin file path or first-seen pins in that check.
- `ljos record check` now fails when the tracker pin file is a symlink to a missing path, instead of reading it as no pin file and falling back to first-seen pins.
- `ljos record check` now folds a voter's seat with the seat slug before the tracker seat pin lookup.
  `alpha/Seat-a`, `alpha/SEAT-A`, `alpha/seat_a` and `alpha/seat.a` are all held to the `seat-a` pin, so a second key can no longer vote beside it under another spelling.
- `ljos record check` now holds a `run-*` voter suffix to the tracker pin file row of that name, when there is one, instead of skipping the seat cross-check for every run.
- `ljos record check` now refuses a pin file that pins one seat to two keys under two of its names, such as `inky` and `inky@39u`.
  The tally now counts a seat once on a question whatever identities and keys it signs under, as it already counted a key once whatever voter ids it signs under, so `alpha/inky` and `beta/inky` are one vote.
  A last part of digits alone is no longer read as an old conversation tag: `alpha/gpt-5` signs as itself and stays apart from `alpha/gpt`, and the tracker keeps a holder such as `ridley-ci-a-555001` whole.
  A pid tag with a letter, as in `inky-39u` or `inky-n9o`, still folds to its seat.
- `ljos record check` now refuses a tracker pin file row whose voter is not spelled the way the check looks it up, instead of silently skipping that pin.
  A BOM or zero-width space before `seat-a`, a non-slug seat such as `Seat-a` or `seat_a`, and a malformed `<identity>/<seat>` row all fail the check.
- `ljos record check` now rejects duplicate JSON keys at any depth, including
  duplicate ballot `stance` fields and duplicate keys under `signature`.
- `ljos record check` now rejects extra keys under `signature`, including nested
  objects that used to be ignored during schema parsing.
- `ljos record check` now requires `replaces` to point to an existing ballot
  from the same voter on the same question instead of silently crossing voters,
  crossing questions, or naming missing targets.
- `ljos record check` now requires the hex suffix of a question id to be lowercase.
  `correct-3f9a1c2e` and `correct-3F9A1C2E` used to tally as two questions, so one voter could cast two counted ballots on one question.
- `ljos record check` reads legacy unsigned ballots by commit order, as the
  push gate does: a ballot counts as legacy when its issue held it in the
  commit that brought the tracker's first pin. It read the committer date
  of the ballot's commit before, which any tracker writer sets.
- `ljos record check` refuses plain-layer `host`, `session` and `agent` fields, and rejects unsigned extra fields in signed entries.
  Timestamps must be UTC hours written `YYYY-MM-DDTHH:00:00Z`.
  A ballot another entry replaces does not count, and two unreplaced ballots from one voter in one hour are an error.
  Every voter id is pinned to one key, including a question's opener and a seat; a run is `<identity>/run-<id>`.
- `ljos record sign_entry` now refuses signer seats that are not the entry's
  named voter or question opener, so test fixtures and generated entries stay
  bound to the identity they claim.
- `ljos release` of an issue the claim graph no longer holds, because a lease reclaim freed it or `ljos complete` ended it, now puts the tracker heading back to the state it had before the claim instead of failing with `release: status ready` and leaving it STARTED.
  A heading another name holds is left alone, and the release still fails when the tracker has nothing of this holder's to hand back.
- `ljos remember` under a runner now stores the agent's own words as an unreviewed note. The prompt does not treat that note as a fact. Words from the person's latest prompt stay a standing lesson. Briefs no longer tell the agent to store its own lesson with `ljos remember --as ROLE`.
- `ljos rule --user` and MCP `ljos_rule` with `user` true record who stands behind the rule, as a seat rule does, and `ljos prefer --user` records it too. Only a user rule with basis `person`, or one promoted from a lesson the pack holds as the person's, is stated in the prompt as the person's standing rule; any other, including a row written before rows carried a basis, is left out of the prompt and listed as unverified by `ljos rules`.
- `ljos rule` at a terminal names the verdict and the pattern, and a failed habitat command says `failed (exit N)` with the program named once.
  With no coding agent found, `ljos onboard` and `install.sh` still make the tracker, the deed store and the memory writer, the installer's force-push line runs from the home directory, and the README says so.
- `ljos rule`, `ljos prefer --deny`/`--ask` and MCP `ljos_rule` now stamp the rule with who stands behind its reason. A rule an agent wrote from a file, a tool's output or a web page is stored `agent-derived` with basis `agent`; before, it went to the pack with no origin and no basis, and the pack filed it as `user-declared`. The person's rule at a terminal, or in their latest prompt, is stored as theirs.
- `ljos rule`, `ljos prefer --deny`/`--ask`, the command rules `ljos prefer` writes, and `ljos prefer --review --keep` treat a writer as the person only on positive evidence: stdin is the process's controlling pseudo-terminal, the process above its session holds that terminal, and the whole chain to pid 1 has no runner and no reparented process. Before, any process not found under a runner was the person, so a tool call that named its parent `systemd`, `sshd` or `init`, left the runner by a double fork, or read from a pipe stored its rule with basis `person`.
- `ljos rules` no longer lists a rule promoted from a lesson as a user rule when the pack does not confirm that lesson as the person's.
  Such a row is listed apart, as `unverified`, with the reason: no hash kept, the pack does not hold the lesson or holds it as an agent's, the record is another one, or the read failed.
- `ljos search`, `ljos island`, the timeline and a sitting's copies of them, and MCP `ljos_search` and `ljos_island` now name each claim by who stands behind it, as recall does, so an agent's note or preference reads as unverified there too; a lesson the pack stored as the person's no longer reads as agent-derived. MCP hit rows gain a `label` field.
- `ljos send` and `ljos reply` in push mode push the sealed log from the detached wake before the wake fires, so a seat on another machine does not read an empty inbox.
  The send does not wait on the push.
  A push that fails is a line in the wake log, and the wake still runs.
- `ljos slot` refuses a seat that already ends with the run id, such as `ridley-review-RUN` for run `RUN`, because `ridley-review` on that run boots as the same seat and the slot could not say which one it vouches for.
- `ljos slot` takes `--key`, the run's public seat key that `ljos cloud up`
  now prints on the `seat key:` line, and signs it into the slot. A run's
  ballot counts only when the tracker pins that key for the run's seat, so
  a writer that pins the seat first under its own key no longer votes as
  the run. A slot written before this names no key and no longer counts.
- `ljos sync --adopt HOST` now adopts each of HOST's logs that this machine
  can open, instead of aborting on the first unopened scope log. The command
  reports skipped scopes and keeps unopened logs in place while still adopting
  and retiring data from openable scopes.
- `ljos sync --adopt HOST` on a machine whose host name is not already a log file name, such as one with a space, now commits this machine's updated log with the deletion of the gone host's log.
  The export wrote `bad host.jsonl.age` while the adopt commit staged `bad-host.jsonl.age`, so peers retired the gone host's copies and never received the adopted atoms.
  The host name is now folded once, and export, adopt and every log path use that name; a log left under the raw name is not imported back as another machine's.
- `ljos sync --adopt-legacy` pinned, and import offered it for, a headerless log that replaced a host's headed one when the clone held only part of that log's history.
  A shallow clone cut by depth or by date, a graft, a replace ref, or a partial clone missing objects could hide the commit with the headed log.
  Where history may be short, import says why and offers no command, `--adopt-legacy` pins nothing, and an earlier pin is not read; `git fetch --unshallow` deepens a shallow clone, and nothing here does it on its own.
  `ljos doctor` names that reason for the host in place of the command.
  A headed log that only one side of a merge carried, as when a peer merged it away with `-s ours`, now counts too; the walk had followed only the side the merge kept.
- `ljos sync --adopt` folds every configured scope of a gone host into this machine's logs.
  A cloud log is no longer left behind when the primary log is adopted.
- `ljos sync --adopt` refuses a scope log that is not a regular file.
  A symlink, hard link, or fifo checked in as a gone host's log is left
  in place, so a link to a machine-key file is not decrypted and resealed
  to the cloud recipients. Ciphertext is passed to age on stdin.
- `ljos sync --adopt` reports a scope log it cannot decrypt and keeps
  that file, and still adopts the host's other scope logs.
- `ljos sync --adopt` says "would adopt" on a dry run, and names only
  scopes that posted atoms. A scope with nothing to post is left out.
- `ljos sync` fails when a log this machine's key is listed for will not decrypt. That used to be skipped as "not sealed to this machine", and a pack that is off hid the failure. A log sealed to someone else is still skipped.
- `ljos sync` keeps the record of what it took from each host under the
  state directory's `sync/` without following a symlinked `sync`,
  `sync/<tracker>` or `sync/<scope>` directory. A linked one is named and
  the import stops.
- `ljos sync` leaves a log with no scope header unread whichever key opens
  it, the cloud identity included. Such a log names no host, so a peer
  could commit one ciphertext under every host's name, or put a host's
  headed log back to the headerless blob it replaced and retire what the
  headed log had added.
- `ljos sync` reads a log only under the name export writes for its host
  and scope. `HOST+PRIMARY.jsonl.age` read as the same log as
  `HOST.jsonl.age`, so an older copy committed under that name retired the
  host's current atoms and shrank what this machine had taken from it.
- `ljos sync` refuses a tracker whose `.ljos` or `.ljos/atoms` is a
  symlink. The scope file, the atoms logs and this host's export are read
  and written under directory fds opened with `O_NOFOLLOW` from the
  tracker root, so a linked directory cannot bring a local log sealed to
  this machine into the import or take this host's log out of the tracker.
- `ljos sync` runs `age` and `rage` with `LC_ALL=C`. A French rage miss (`Aucune clef correspondante`) is the same not-for-this-host result as the English text, so import continues with the remaining hosts.
- `ljos sync` seals a header naming the writing host and scope into every
  atoms log, and import and `--adopt` refuse a log whose header names
  another host or scope than its file name. A log sealed only to this
  machine and committed as a cloud log is no longer resealed to the cloud
  recipients. A log written before the header opens only with the cloud
  identity for scope cloud; any other is named and left unread until its
  host exports again.
- `ljos vote` now refuses a second ballot on an issue from a shell that already cast on it as another session, so setting `LJOS_SESSION_ID`, `CURSOR_AGENT_ID`, `CLOUD_AGENT_ID` or `CURSOR_CONVERSATION_ID` to a new value between casts no longer makes a second voice.
  A cast records its calling shell, and the harness whose hook saw its line, by pid and start time in the runtime directory; a harness may still cast once per subagent of one root session, and only another root session from it is refused.
- `ljos_guard_check` now asks `ljos-policyd` what the hook asks it: the
  line as `bash -c LINE`, each program a heredoc feeds (`python3 - <<'PY'`)
  and each download pair. It sent only the words of the line, so it
  allowed a `python3 -` heredoc that runs `os.system('rm -rf /')`, and
  `(cd project; make clean-all)`, both of which the hook refuses.
- `ljos_start` after a Write keeps the issue the Write hook claimed instead of closing it and filing another.
  A runner starts each hook through `sh -c`, and the hook recorded that shell, which exits with it, as the runner that owns the sitting.
  The MCP server's next call then finished the sitting as orphaned, so the start found nothing held and claimed a new issue.
  On the willma-20261021g seat replay this happened on all four harnesses: a Write claimed work-X under `free-claude@mf`, and `ljos_start` closed it and claimed work-Y under `free-claude`.
  A sitting is now owned by the first process above the hook that is not a shell or a wrapper.
- `mail_wake_push` no longer needs Linux user namespaces. The test harness
  sets `LJOS_TEST_HOSTNAME` for each simulated machine and runs `ljos`
  directly, so CI runners that block unprivileged user namespaces still
  exercise push-before-wake behavior end to end.
- `mcp_forward` retries a spawn that fails with "Text file busy" for about a second, and the tests close and fsync a script before they run it.
- `prepare_tracker` now ignores exported `GIT_DIR` and `GIT_WORK_TREE` when it prepares another path, so it initializes and configures that tracker without rewriting an unrelated dotfiles repository.
- `same_holder` now refuses malformed holder names that end in `@`.
  This keeps `seat` and `seat@` from matching as one holder.
- `server.json` carries the crate version, and CI refuses a copy whose version is not the one in `Cargo.toml`.
- cloud guard now evaluates `git send-pack` bulk flags (`--all`,
  `--mirror`, `--tags`) against local refs so hidden `cursor/*` branch or
  tag updates are refused the same way as guarded `git push` bulk forms.
- cloud guard now inspects `gh api` git-ref writes (REST and GraphQL) and
  refuses cursor ref creation, ref deletion, and force updates, including
  payloads read from `--input` and `@query` files.
- cloud guard now parses `!` git aliases through nested shell command
  expansion so aliases that hide `git push` under `sh -c` cannot bypass the
  `cursor/*` push refusal.
- gh, curl, wget, and httpie are now an allowlist when they talk to
  GitHub. A resolved GET or HEAD passes. Any other method, a body, or a
  flag whose meaning is not on the command is refused. ljos itself only
  reads. A program that calls the API on its own is outside this filter.
- ljos asks the pack to rank by relevance only when the pack says it
  can. It reads `search.ranks` in the pack's status, once a process,
  and sends `rank=relevance` to a writer that lists it. A writer that
  does not, or does not answer, is searched as before, and ljos adds
  the stored memories that best match itself.
- ljos recall, the MCP recall tool, the sitting and the persona brief now fence the tracker's issue text and logbook notes as unverified tracker text instead of pasting them unmarked.
- omp blocks a command when `ljos hook` writes a deny and exits non-zero.
  It reads the hook's JSON on any exit, and exit 2 without JSON is a deny
  with stderr as its reason. Before, omp dropped the deny a failed gate
  writes and ran the command. On Codex, which shows no prompt, a force
  push no longer says the agent asks first; it says the push needs the
  person's approval.

  The README lists the runners whose hook blocks a denied command apart
  from the ones it serves over MCP, and names hermes and Amazon Q with
  the MCP ones, since onboard writes no hook for either. The integration
  page says a failed Claude Code or Cursor gate blocks the call and that
  Codex turns an ask into a deny. It says which guard faults
  `ljos hook --runner` denies, with `GUARD_TIMEOUT` as the 5 second limit
  `LJOS_GUARD_TIMEOUT` overrides, that an `ljos-policyd` that is not
  installed is not one of them, and that Claude Code, Codex and Cursor
  still let a command run after a guard fault, with a note.
- opencode-style workdirs now resolve lexically, so `lnk/..` stays at the
  parent path instead of being canonicalized through the symlink target first.
  The resolver also expands a leading `~` in workdir overrides.

### Documentation

- A prompt cost of $0.00004 is the arithmetic of about 900 input tokens at $0.042 per million, with output free.
- Codex integration docs now state that `shell_command` payloads carry only
  the command and session `cwd`; there is no `workdir` override field.
- Documented that Codex `shell_command` hook payloads carry the command and the
  session `cwd`, but no `workdir` override field.
- Record schema docs now spell out that first-seen voter pins are scoped to one
  `record check` run unless pins are ingested durably, and clarify how unsigned
  payload canonicalisation is enforced.
- Remember and prefer now say that any other text is refused, so those
  descriptions are shorter. The measured core set is 8,062 o200k_base
  tokens and all fifty tools are 16,658.
- The README and `docs/cloud-vm.md` say that `ljos cloud up` and `install.sh --cloud` arrive in the next release, which the install line and the job step fetch; the latest release has neither. Until then both work from a checkout of `develop`, where `sh scripts/install.sh --cloud` builds that tree's `ljos`.
- The README counts twenty-one core MCP tools at 8,062 o200k_base tokens and all fifty at 16,658.
  CI checks that count against a fresh `tools/list`.
- The README's cost of the default MCP tool set is the measured count: 7,702 tokens for the twenty core tools against 16,298 for all forty-nine (o200k_base, `initialize` instructions plus the compact `tools/list` result). It said about 8,200 and 17,800.
- The README's five-minute paste runs `export PATH="$HOME/.local/bin:$PATH"` after the install, so the `ljos` lines after it find the program in the same shell. It says the installer adds that line to the shell profiles from the next release on, and that `--no-modify-path` leaves them alone.
- The README, the docs and `ljos onboard` tell Codex users to trust each ljos hook in `/hooks` before the guard runs, and again after an upgrade. `ljos doctor` reports an untrusted hook. Until then only the rm, git and find shims guard Codex's commands.
- The `sit` playbook no longer says lessons are `ljos remember`. It says that under a runner `ljos remember` is an unreviewed note that the person reviews.
- The cloud VM doc no longer says `cloud up` builds `packset-embed` from source off a cloud VM.
  Off a cloud VM the command downloads no encoder. `sh scripts/install.sh --cloud` sets `WITH_EMBED=0`.
- The cloud VM docs no longer say `ljos consensus` counts a run's ballot only under its slot. The push gate, and the push cite line `ljos consensus` prints, count a ballot the run casts as its own seat when its slot verifies; a ballot cast `--as` a persona is left out before any slot is read, and the settle reads no slot.
- The cloud guard docs now say which check sees which write. The shell
  guard filters the command as written. The pre-push hook is the git
  backstop. A program that calls the GitHub API with the token needs a
  server-side ruleset.
- The cloud guard docs now say which pushes this machine does not
  see. A script, make, python, or dulwich is not on the command
  line the shell guard reads. `--no-verify` inside a script skips
  the pre-push hook. `git send-pack` does not run that hook. A
  server-side ruleset is what stops those.
- The explanation now says plainly what the kept prompt does not stop: a process of the same user that runs outside the seat guard, not as a tool call, can still forge the person's words. A person basis is strong only against writes made through tool calls the guard sees.
- The explanation now says who keeps the person's prompt (only a hook the runner spawned), what each runner's own record of the turn is and whether it is required, that an MCP `_meta` session only narrows the check, and what remains: code of the same user that forges the runner's transcript as well as the kept prompt and its bind, or runs outside the guard. It is a check on tool calls, not a sandbox.
- The explanation of the person's words now says what the prompt check and the seat guard hold to: a whole clause with its negations, a session read from the runner above and never from the asking process, paths however spelled, interpreter code, every file tool argument, and ljos through ssh. The residual stays: a process of the same user outside the guard can still write the kept prompt and its bind.
- The explanation page links the committed correlation-history and forgetting runs beside the 6.4 point gap and the 0.947 retrieval figure.
- The getting-started search sample is the two lines release ljos printed for `ljos search "which fusion"` after that remember and prefer.
- The habit sample uses source `example-0001` and is marked as an example, so it is not a real job id.
- The howto force-push sample is the ask the hook prints, with rule `git-force-push`.
  The doctor sample is the ljos row taken from ljos 0.32.1.
  `ljos protocol` lists the sitting sections in the order `ljos sitting` prints them, reclaim before claim.
- The install paragraph names packset, deedar and consensus as the repositories that gain a build attestation on their next release, which the installer counts as not published until then.
- The integration docs now describe a guard fault as a deny: a policyd
  that is installed but hangs, crashes, or prints no verdict, and a
  relay that cannot read its input. A missing `ljos-policyd` still
  allows, with a note, unless `POLICYD_REQUIRED=1`.

  The same docs now document `LJOS_GUARD_FAIL_OPEN=1` as the temporary
  escape hatch that restores the previous allow-with-note behavior for
  a fault.
- The ljos_note tool text and the session-end extractor's documentation now say that a note is stored as an agent note in memory, read by its stored origin, and that its logbook line is fenced as unverified tracker text.
- The note tool now says recall labels an unverified agent note, so its
  description is longer. The measured core set is 8,062 o200k_base tokens
  and all fifty tools are 16,658.
- The protocol and the ljos skill say how a voter changes its ballot: withdraw it with `ljos vote ISSUE --withdraw --as NAME`, then cast again.
- The protocol and the ljos skill say the tracker commit lock is `.ljos-commit.lock` at the top of the worktree, not a file in the git directory, and that a stale `index.lock` is named with the `rm` that clears it.
- The protocol and the skill say `ljos doctor` names the adopt command
  once per host only when history is complete.
  Otherwise it names the host and why no command is offered.
- The protocol describes extra scopes under `[scopes]`, and `cloud up` documents `LJOS_WRITE_SCOPE=cloud`.
  A primary scope named `cloud` is refused, and `default_scope` cannot name an extra scope.
- The protocol now states what ballot binding defends against: an honest but careless agent voicing a panel from one session, not a process running as the same user that rewrites the tracker, the seat keys or the runtime directory.
  It names what is still not caught, and no longer says a ballot carries a nonce of the process: the second field is `-`, or `seat` when only the seat named the session.
- The protocol text names the same `ljos doctor` line as the skill a harness
  loads, so a headerless log's `ljos sync --adopt-legacy HOST` command is in
  both.
- The protocol, the remember skill and the panelist now tell an agent to keep its own lesson with `ljos note`. `ljos remember --as` is no longer how that lesson is stored.
- The record check and holder docs now say what the code does: `check_entries` lists the errors it raises and no longer calls a same-hour pair an error, the tally rule says a seat is the part of the voter id after `/` whatever the identity, and `holder_parts` names the numbers, runs, judges and subagents it keeps whole.
- The record schema docs now say what `ljos record check` enforces: duplicate-key rejection, exact stances, the `replaces` rule, strict signatures, the seat cross-check by seat slug, and the pin file rules.
  The first-seen pin sentence now says it applies only when neither the voter id nor its seat has a pin file row.
  The docs also state that the check reads one directory and cannot tell a withdrawal file was deleted, so a deleted withdrawal revives the earlier ballot.
- The reference says a lesson written again after an amend or a rebase closes the one that named the replaced commit.
- The reference says which commits `Stop` and `SessionEnd` cite when the git hooks are not installed.
- The seat protocol now names the writes that are refused: a runner with no prompt hook, `LJOS_CAPTURE=off`, a paraphrase, and a prompt older than an hour. The four-word match is unchanged, and the docs say so.
- The skill and the howto say that each reviewer casts its own ballot from its own run, with ljos installed by `ljos cloud up` and the tracker synced through the shared tracker repository.
  They also say that persona and Jev ballots are advisory, and that the push gate counts seat ballots from another seat.
- The skill and the howto say that each reviewer votes from its own subagent or run, and that a subagent's ballot binds the parent session plus the subagent, `SESSION:AGENT`, from Claude Code's `agent_id` or the Codex thread, while an opencode subagent is a session of its own.
- The skill, the sitting's `== panel` step and the howto say that each persona brief goes to its own subagent or run, which casts `ljos vote --as NAME` itself, and that the seat does not cast `--as` for its personas.
- `CONTRIBUTING.md` now names `ljos-policyd` among the programs the workspace tests need on `PATH`, built from its `develop` branch with phronesis `v0.2.5` linked, as CI builds it.
- `CONTRIBUTING.md` now names the programs the workspace tests need on `PATH`: `age`, the `packset` CLI and `packsetd` from packset `develop`, and `deedar` at the pinned commit.
- `docs/design/mail.md` describes the opt-in mail tools, the `mailgate` user that alone can send, and the send gate: the approval file, the ballot, the refuse-only checks and the owner's yes.
- `ljos onboard --example` says the MCP command is `ljos-mcp` when that name on PATH is this install, and the absolute path otherwise. The example used to say an absolute path would be the machine that wrote the file.
- `record-v1.md` now says, under "What the check cannot see", that a ballot is not bound to a tracker and that a new identity signed with a newly minted key is a new voter, so only the pin file limits how many voters one party can cast.

### Miscellaneous

- A `crates/ljos-cli/vissue.toml` left by a test run is no longer in the source tree.
- A cast refused because the session already voted opens with `already cast a ballot on ISSUE (SESSION)` whether or not it names the same voter, so a reader or script matching that refusal still finds it.
- A cloud finish with a signing key is checked against the pinned deedar.
  `check` and `import` accept the bag when finish leaves the export link sidecars untouched.
- A dated search lists the pack before it queries, so the one-shot test counts that listing and then the search that carries the date and the rerank.
- A dotfiles checkout test passes `GIT_DIR` and `GIT_WORK_TREE` only to the git commands it starts.
  Other tests in the same process no longer inherit that checkout.
- A test casts eight persona votes and four notes at once into one git tracker checkout, through vissue-core, and checks every write is committed, the tally counts each ballot, and no `index.lock` is left.
- A test now holds that a run's ballot under its slot is left out while
  the run's pin is only in the working tree, and counts once the pin is
  committed.
- A test now holds that on a tracker that is not a git repository a run's
  ballot counts under a slot from a seat on the standing list, when the
  run is pinned to the key its slot names.
- CI checks only the commits a push or a pull request adds, and
  rejects one whose author or committer is not Rohit Goswami
  <rgoswami@ieee.org>, or whose message has a Co-authored-by or
  Signed-off-by trailer or a generated line.
  `scripts/identity-gate.sh` runs the same check on a local range.
- Push-gate path tests now force `LJOS_GUARD=on` so they still exercise lease-push parsing under a temporary test `HOME`, and the MCP guard-check test comment now matches the tool's allow-versus-error behavior.
- Restored the three-argument `tcb_answer` API as a wrapper around the cwd-aware
  entry point (`tcb_answer_in`), so existing callers keep compiling while the
  hooked path still passes an explicit directory.
- The `ljos cloud up` test for Ubuntu's `case $-` interactive guard carries that `.bashrc` itself instead of reading the host's `/etc/skel/.bashrc`, so it runs the same on every distribution.
- The bookkeep config test and the guard shim test no longer treat a
  temporary directory that lives under the login home as that home's
  own config or as a permanent seat.
- The cloud guard's toml header parse takes a missing bracket as an
  early return, which is what Rust 1.97 clippy accepts, and a header
  it could read still parses as before.
- The cloud-finish import test and CI build the deedar commit named in `.github/deedar-pin`.
- The deedar refusal test now isolates PATH edits under the shared test
  environment lock and reuses the existing environment restore helper.
- The guard and booking tests put their seat homes outside the temp roots wherever the checkout sits, so the suite passes from a checkout under `/tmp`.
- The guard shim test that checks a deny is refused now pins `LJOS_GUARD=on` and clears `LJOS_OFF`, so it passes when the suite runs with a temporary `HOME` or `LJOS_OFF` set.
- The held-checkout sync test passes `cargo clippy -D warnings` on Rust 1.97, which rejects a cloned path made into a one-element slice.
- The isolated test config directory is removed when the test process exits, and the connected-seat test runs in a temporary home with `LJOS_GUARD=on` in place of a directory in the build tree.
- The job-folder test no longer exports `GIT_DIR` for the whole process.
  The origin lookup still receives that directory on its git command and drops it, so `-C` stays on the checkout.
- The mail wake harness probed a free port and then let packsetd bind
  it. Another test could take that port in between, so reply talked to
  a daemon that had already exited.
- The origin-refusal writer test checks the atom posts in order, so a search from a parallel test landing on the same mock server first does not hide the retry that drops `origin`.
- The panel test now casts a ballot from a seat off the standing list with
  no slot, and checks that the push cite leaves it out by name while the
  listed seats still decide.
- The pin-ignore test names its decoy `GIT_DIR` on the git command and then drops it.
  Other tests in the same process no longer inherit that directory.
- The pinned deedar helper builds once per process into a directory named by the sha in `.github/deedar-pin`, and leaves an existing tree in place so two finish-bag tests cannot wipe each other's build.
- The preference migration test's pack now asks for this seat's token. A listener that answers without one does not list a migrated preference as the person's.
- The search label test now writes a rule from a pipe and one from the person's terminal and pins the exact label of each, so it fails on a writer that leaves a rule without a basis. The fixture-only test it replaces passed before the writer was fixed.
- The second-issue lesson check again requires a rewrite under that issue's own title.
- The shared-override record test signs both runs while it holds the environment lock, hides `GITHUB_RUN_ID` for that span, and restores the previous values when the guard drops. Record tests also clear a leaked `CURSOR_AGENT_ID`.
- The test suites no longer inherit the agent runner that launched `cargo test`. The shared test environment lock and the user rule harness clear `CURSOR_*`, `CLAUDE*`, `CODEX_*` and the other runner and session variables, so the suites pass with or without them.
- The test that two runs sharing an override seat key sign with different keys now passes on GitHub Actions, where the run id used to name both runs.
- The tracker row test gives a fresh unpushed commit ten minutes before calling it stuck.
  A parallel git suite can spend longer than five seconds inside that check.
- The two cloud finish bag tests build the pinned deedar one at a time, so neither clears the cache the other is building in on a fresh checkout.
- The upgrade test installs the published v0.32.1 binaries and stores a preference through each old path, then checks that a prompt hook and a terminal prefer still show and a prefer run under a runner stays unverified until a terminal keeps it.
- The write-side tests run their ljos and ljos-mcp processes in the seat's temporary home, and the tracker file a run left in the crate is gone.
- Two seats on packsetd exchange mail in the test suite: group add, send, inbox, read, reply, interrupt, and the receipt `ljos hook --prompt` writes for a shell and for Cursor.
- ljos builds on packset-client and packset-core 0.16.0, and `ljos doctor` marks an `ljos-policyd` older than 0.5.0 `no`.
  The release workflow builds `packset-embed` 0.16.0 and the bookworm archive installs packset 0.16.0.
  The README and the tutorial name the same floor.


## 0.32.1 (2026-10-20)

### Fixes

- Bound transcript prompt reads and retain the stored prompt during tool calls.
  Recognize user messages inside event envelopes so long conversations do not stall command hooks.
- Let an initialized connection reach command policy without a historical tracker claim.
  Keep automatically filed claims with their hook conversation when a runner shares hold records.
- MCP memory writes hold what the person stated. The server instructions no longer say `Call ljos_note and ljos_remember freely while you work.`: `ljos_remember` and `ljos_prefer` are for a fact or a preference the person stated in this conversation, never for a claim read in a file, a tool's output or a web page, and `ljos_note` keeps the agent's own working notes. An MCP write of words from the person's latest prompt, as the prompt hook kept it, is stored as theirs: origin `user-declared`, `via:prompt` and `source.basis` `person`. Any other MCP write is the agent's, with origin `agent-derived` and `source.basis` `agent`; before this, an MCP `ljos_prefer` was stored as `user-declared` whatever its source. The prompt note reads preferences first, then the person's words, then other claims, and working notes last, and labels a note `agent's working note` and a claim in the person's words `the person's words`.
- Record the connected identity before admitting a tool request when initialization and the first request arrive together.
- The sync that `ljos sitting`, `ljos finish` and the MCP server's exit run follows `LJOS_TRACKER_GIT`. It pushed the whole tracker branch with this machine's sealed memory log, so a ticket commit that `LJOS_TRACKER_GIT=commit` had kept local left with it. Under `commit` the log is committed and nothing is pushed; under `off` the tracker is left alone, and when a scope is set up the sitting and finish print one line saying so. `ljos sync` run by hand pulls and pushes as before. The README now says that a tracker with `.ljos/sync.toml` gets the sealed log from each finish.
- User rules leave the machine only through a tracker whose `.ljos/sync.toml` names a scope, sealed with age to that scope's recipients as `.ljos/user-rules/<host>.json.age`. Every rule write and session start pushed `.ljos/user-rules/<host>.json` in plaintext to any tracker with a git remote, with no scope file and no notice. A tracker with no scope now gets nothing, and the rules already in its checkout are still read. A plaintext file another host left is still read, and this machine's own is removed in the commit that seals it. The pass at a write and at session start follows `LJOS_TRACKER_GIT` (`commit` keeps the commit local, `off` leaves the tracker alone), and `ljos prefer --user --sync` pushes as asked. `LJOS_USER_RULES_SYNC=off` still stops it. The reference lists `LJOS_USER_RULES_SYNC`, `LJOS_USER_RULES_TRACKER` and `LJOS_USER_RULES_SYNC_MS`. A plaintext file already pushed stays in the tracker's git history; rewriting that history is left to the person who owns the remote.
- `ljos_guard_check` gives its verdict whatever seat the call names. Codex onboarded with `LJOS_SEAT=codex` on a tracker whose only codex claims were held as `codex-vax0`, so the server refused every tool call with "no seat named codex", the guard included, and listed thread ids as seats. The seat check now applies only to tools that act under a seat: claims, sittings, mail and writes. The guard, the approval form and the reading tools run without one. A harness id is that harness's seat, or its one seat on the tracker when there is exactly one. A name from `LJOS_SEAT` or `VISSUE_AGENT` is the seat even before any claim names it. A `CLAIMED_BY` holder in the `SEAT-TAG` form of ljos 0.31 counts as its seat, and a bare thread id counts as none. A name the call made up is still refused, in one line that names the seat it most likely meant and at most four others, with no ids. Mail to an unknown seat lists seats the same way.
- rustls 0.23.46, rustls-webpki 0.103.16 and webpki-roots 1.0.10 fix the TLS advisories open against ljos 0.32.0, and the build no longer trusts the emSign ECC Root CA - G3 root.

### Miscellaneous

- CI runs `cargo deny check` with every section, where it ran the license section alone. Each ignored advisory is named with the crates that bring it in, and `deny.toml` bans every webpki-roots release that still ships the emSign root.
- ljos-hud depends on claimdag 0.8, built on capnp 0.27, which clears RUSTSEC-2025-0143. The work.bin format is unchanged, so a graph claimdag 0.7.1 wrote still opens.


## 0.32.0 (2026-10-20)

### Features

- `ljos hook --prompt --text` prints what the prompt hook would tell the agent as plain text, so a person can read the recall without `jq`. It runs the same hook, so mail it shows is still marked read.
- `ljos_recall` is in the default tool set, and it answers like `ljos recall`: the issue's working set, then its memories, notes included. opencode sessions called `ljos_ljos_recall` and got "unavailable tool", so a note was stored and never read back.
- install.sh checks build attestations without a signed-in gh. It fetches each tarball's attestation from GitHub's public API and verifies it offline with gh 2.49 or newer or with cosign, reading the answer with jq or python3. A signed-in gh still verifies on its own. Each tarball prints one line, and one closing line counts how many verified, how many have no attestation and how many could not be checked; a found attestation that does not verify is a warning. `--attest` now also takes cosign, and stops on a missing attestation as well as a failed one. The installer no longer prints `checksum ok, no attestation verified` seven times.

### Fixes

- A `git push --force-with-lease`, with or without `=REF:SHA` and `--force-if-includes`, now asks when it lands on `main`, `master`, `develop` or a branch the checkout protects (`protected_branches` in `ljos.toml` or `.ljos/gates.toml`), and runs onto any other branch. Before, every lease push ran. With no refspec the guard reads the current branch's upstream, and asks when it cannot tell. `--force`, `-f` and a `+refspec` ask as before, and so does a "Never force push" preference on a lease push that names `main`, `master` or `develop`. The `git-force-push` reason is now one sentence, `it rewrites shared history, so the agent asks you first`, in place of a 221-column one that recommended `--force-with-lease`. A refusal the phronesis law words as `git-force-push: ...` asks as well, where it was a deny. The hook also sends `ljos-policyd` each download beside each later command on its line, so `curl -o i.sh URL && sh i.sh` is judged as one call; with ljos-policyd past 0.4.4 that line is refused.
- A ballot whose choice holds a space is still one ballot per session. The signature note was read as whitespace fields and a choice like `beside binary` gave too many, so the note did not parse and the next cast from that session went through. In the panel eval on 7e21d3c one session cast `beside binary` and then `xdg home`, and the tracker kept both ballots. The note now finds the key and signature by their hex lengths from the end, and the cast gate reads the session from any ballot note, so the second cast is refused and the tally counts the first.
- A git hook and `ljos_start` in one checkout no longer hold under two names that miss each other. The hook booked `free-claude-8h` and `ljos_start` booked `free-claude`, so `ljos_start` while the hook's issue was held filed a second issue, and a Write after `ljos_start` did the same. Holders of one seat (the bare seat, `SEAT@TAG`, and an older `SEAT-TAG` such as `free-agy-aak`) now count as this seat when ljos looks for the issue held in a checkout. A whole seat on the tracker's list, such as `eessi-bot`, stays its own seat. A named commit takes back its issue from another name of the same seat.
- A push run outside a git checkout is not an evidence denial. There is no tip and no gate file to check, so a plain push is allowed and a force push asks, the same way it does in a checkout that sets no test command.
- A recall sent right after a note lists that note. The MCP server answers each call on its own task, so a note and a recall a client sends together ran at once. In the willma matrix on 7e21d3c, opencode sent `ljos_note` and then `ljos_recall` on one issue 7 ms apart in cells w7 and w11, and the recall answered before the note was stored. The note, remember, prefer, forget, start, sitting, claim, finish, release and deed tools now run in the order they reached the server, and a recall, search or timeline waits for those that came before it. Reads still run beside each other, and the guard, approval and every other tool wait for nothing.
- A subagent conversation (`sand-sub...`) with no `LJOS_SEAT` is no longer the seat `grokbot-sand-sub`. Its git hook books nothing and prints `not booked` with the `LJOS_SEAT` to export, and the Grok Bot env file says the same. ljos-mcp writes holders as `SEAT@TAG`, not `SEAT-TAG`. Older holders such as `inky-n9o` and `disk-saver-t4t`, and the old seat name `ljos-bot`, now count as their seat (`inky`, `disk-saver`, `ljosbot`), so any conversation of that seat can finish or release their claims.
- A subagent with no seat is told so where it sees it. The git hook prints `not booked` on stdout, which the installed hook shows (it drops stderr), and `ljos seat`, `sitting`, `claim`, `note`, `finish`, `release`, `complete`, `file` and `deed` refuse with the `LJOS_SEAT` to export instead of holding under the process tree (`bash`).
- A tag push (`git push --tags`, `--follow-tags`, `git push origin v1.0.0`) runs with no prompt, as a branch push does. A force, a remote branch or tag delete and `--mirror` still ask.
- A temporary HOME says what it turns off. ljos treats a HOME under /tmp as a test sandbox and starts no memory writer there; a memory command used to fail with the bare connection error, and now says the writer does not start on its own under a temporary HOME and to run `packset ensure`. A temporary working directory under a real HOME no longer stops the writer. `ljos doctor` shows a `guard` row (`warn`) whenever the guard is off, from `LJOS_OFF` or a temporary HOME or working directory, and install.sh says so at the end of an install into one.
- An agy `write_file` files the claim. agy 1.4.3 writes a new file with `write_file`, and that name was not on the hook's list of file tools, so the write booked nothing and the claim came later with `edit_file` or a shell command. In the willma matrix on 7e21d3c this was cells w4, w8 and w12, where the after hook answered `{}` on the write. opencode 1.19.3 names its file tools `write`, `edit` and `apply_patch`, and Codex uses `apply_patch`; those already claimed.
- First-run output says what it means. `ljos policy` no longer prints `argv law. ljos-policyd is the TCB when present. Reloading a pack is not a check.` before every check. A push the gate stops reads `ask: a force push rewrites shared history; the agent asks you first (rule: built-in push)`, and a runner's reason ends `(ljos rule NAME)` instead of `(seat rule NAME)`. Recalled memories sit under `Remembered, oldest first (ljos search for more):`. A ready `ljos doctor` says `ljos is ready: memory, tracker and command guard work. Optional extras: ljos doctor --all.` `ljos remember` and `ljos prefer` put the id and review time on a second line, and `ljos --help` opens with what ljos is and where to start.
- The guard sees the file an agy `write_file` or `edit_file` names. agy 1.4.3 sends the file as `Path`, a field the hook did not read, so those calls reached the guard with no path and a write to the seat's own binary or hook files went unchecked. `Path` is now read beside `TargetFile`, `AbsolutePath`, `FilePath`, `file_path` and `path`.
- With no rule of its own, the guard lets `rm -rf` of a build or scratch directory (`target/`, `node_modules`, `dist`, `./tmp/out`) and a `find -name '*.pyc' -delete` cleanup run inside the working tree, as ljos-policyd does. The home, the root, `.git`, a path that climbs out and a line that can `cd` away first are still refused.
- `git commit --amend --no-edit` runs: it keeps the last message, so the commit gate has no new text to read. A new message (`-m`, `-F`, `--trailer`) is still checked for an AI trailer.
- `ljos consensus` takes the panel round from the tracker. Each filing reading writes a `panel-round: N` note and the closer reads the highest, so a panel titled with its question closes as settled-with-notes at round 3. In the panel eval on 7e21d3c twelve such panels read as round 0, and the four split ones stayed TODO after three rounds. A `panel rN` title from `ljos workflow --round` is still round N, `--no-file` sits no round, and a closed issue is not closed again. The source comments now say that `ljos consensus` files `Dissent holds` children and that `fix:` children come only from `ljos workflow --round`.
- `ljos_note` no longer files a STARTED issue titled with the note text. The note goes on the issue named in the call, else the one this seat holds. With neither, the call is refused and says to pass `issue` or call `ljos_start` first.
- `ljos_remember`, `ljos_prefer` and the other MCP write tools no longer file or claim an issue before they run. Their text used to title a new STARTED issue when the seat held nothing. Now a call joins the issue this seat holds in the checkout, and with none held it runs unattached and `book.log` says so.
- `ljos_send` and `ljos_reply` no longer file a claim. ljos-mcp counted mail as a write, so the first message a seat sent filed an issue titled by the message text, such as `Hourly poke from Inky: ...` (ljos-6l1n, workspace-57pt).

### Documentation

- The README now gives the measured cost of the default MCP tool set: 8,201 tokens for the twenty core tools against 17,830 for all forty-nine (o200k_base, `initialize` instructions plus the compact `tools/list` result from aea765d). It said 6,100 and 15,000 from the seventeen-tool set.
- The howto doctor sample is the real `ljos doctor --all` output on 0.31.0, with vissue-cli 0.21.0 and packset 0.15.3, paths replaced by placeholders.
- `docs/demo/demo.sh` records the README demo with no step off screen. Its fresh HOME is made outside /tmp, so the writer starts on the first memory command and the guard is on, and the recall step uses `ljos hook --prompt --text`. The README's sample preference is now `Push a tag with git push origin TAG; --follow-tags skips lightweight tags.`, which fits a 100-column terminal.

### Miscellaneous

- A release tag builds and checks the docs but does not deploy them. The github-pages environment takes main only, and refused the v0.31.0 deploy; main is moved to the tagged commit first, so its deploy publishes the same docs.
- `ljos doctor` marks an `ljos-policyd` older than 0.4.5 `no` and exits 1, naming `rustup update stable` and the reinstall.
  The floor was 0.3.0, so 0.4.4 still read as ready even though it lacks the guard fixes in 0.4.5, the release linked to phronesis 0.2.4.
  The README and the tutorial name the same floor.


## 0.31.0 (2026-10-19)

### Features

- A hook call finishes a sitting that has been idle for two hours, or for `LJOS_SITTING_IDLE`, and clears the prompt so the next sitting does not keep the old title.
- Grok Bot seat names are read from `seats.toml` under the config home by `ljos seat`.
  An unknown conversation id becomes `grokbot-` plus its first eight characters.
- The guard shim prints held claims, recall, and unread mail on the first git command of a conversation, and not again in that conversation.
  A booked commit prints one recall line when the sitting's title bears on the message.
- The static guard refuses a commit that carries a machine trailer, and a commit that omits Signed-off-by when the gate config sets `signoff`. The author and committer are checked only when `name` and `email` are set in `.ljos/gates.toml` or in the user, repository, or seat `ljos.toml`. With none of those set, any author may commit. A push needs a passing `ljos evidence record` for that exact tip when the config sets `test`, and a plain push with that record needs no approval. With no `test` command a plain push is allowed, and a force push still asks. A merge needs green CI on the head sha and every Cursor Bugbot thread resolved or replied to. The hook, a wake, and `ljos guard_check` share that check.
- `ljos git-hooks install` writes post-commit and post-rewrite hooks that book the commit through `ljos hook --git-commit`.
  The hooks chain to each repository's own hooks, skip the tracker and a claimdag or deedar store, skip the middle of a rebase or merge, and skip a Claude Code, Codex, or opencode session.
  An amend and a rebase are booked once, a hook failure never fails the commit, and `LJOS_OFF` turns the hooks off.
  `ljos onboard --harness grokbot` installs them.
  The MCP server tells the client to pass `seat` and that commits are booked automatically.
- `ljos note` queues the memory copy when the pack is off or not answering, and the next note that reaches the pack, `ljos finish`, or `ljos doctor` sends it. A refusal is not queued, the same note waits once, and packsetd answers a resend of the same issue and text with the claim it holds.

### Fixes

- A Codex 0.163 transcript stores the exit on the command item's id, and otherwise in the line Process exited with code N, so a failed command is kept for the lesson.
- A Codex command whose payload has no exit code takes that code from the transcript, so a failure is kept for the lesson and a commit whose exit cannot be read is not cited.
- A Stop or SessionEnd in a checkout the book has no row for opens no row. An MCP server that answered only guard checks no longer leaves a seat book row with no issue when it exits.
- A checkout whose origin matches no tracker project files under the origin repo name; the folder name is used only with no origin.
- A commit that names an open issue now takes over the issue a hook filed for the sitting's first edit, when that issue has no deed yet and the tracker has it STARTED under this holder. The row moves to the named issue with the sha in `deeds`, and the placeholder is cancelled with a log line, from STARTED or from the TODO a claim graph's release leaves. On willma-20261018d, 10 of 12 cells left that placeholder STARTED with no deeds and the named sha under `elsewhere` (w1: work-ipph next to work-wf02). A filed issue with a deed, or one another holder has, stays as it was.
- A commit whose runner omits the exit code is cited when the payload or the checkout shows it landed: a git operation, a `[branch sha]` line, or HEAD moving past the sitting's start. Every commit since that start is a deed, including one a compound command wrote. An explicit non-zero exit is still a failure, and it does not finish the sitting or replace that start. The same command exiting 0 later, and the git hook, cite the new commits on the claim already held.

  The first prompt of a session says the user rules, the claims already held, and the lessons already stored before it asks whether the search model is loaded. The warming line is what a prompt with nothing to recall hears.

  A transcript that never answers is read without blocking the hook.
- A commit whose subject or body names an open issue the tracker holds, such as `ljos-policyd-32fx: ...` or `Fixes ljos-xsor`, is claimed and cited on that issue, and no new issue is filed. A word shaped like an id that is not in the tracker, or an issue already DONE or CANCELLED, is not a named issue. At the end of the sitting the named issue is released back to its earlier state, not closed.
- A git hook books a commit as the resolved seat: `LJOS_SEAT`, then `seats.toml` through `CURSOR_CONVERSATION_ID`, then `grokbot-` and the first eight characters of that id. The claim, the deed and the tracker record are that name, passed on the child command. The process tree is not a seat, and a commit with none of those names is left unbooked.

  A project matches a checkout by a recorded remote only: a `#+REMOTE:` header, a `remote` field in the project's metadata, or the repository name in that remote. A URL in a note does not.

  `ljos git-hooks install` writes a stub for every standard hook. The stub runs the repository's own hook, or the previous `core.hooksPath`, with the same arguments and stdin. `post-commit` and `post-rewrite` still book. `[git_hooks] skip` in `ljos.toml` adds paths that are not booked, and a reinstall keeps that file.

  `pre-commit install` follows `core.hooksPath` and would replace a stub. Install it into the repository first: `git config --local core.hooksPath "$(git rev-parse --git-dir)/hooks"`, run `pre-commit install`, then `git config --local --unset core.hooksPath`.
- A git hook claims an issue whose project contains hyphens or digits. The id is the project, then a hyphen, then the base36 suffix vissue mints.
- A git hook holds under the holder `ljos seat` prints, `SEAT@TAG` when a session id names the conversation, which is the name `ljos claim` takes. The hook used to hold under the bare seat. A holder taken from the process tree is not used. Release, the tracker's claim and a takeover treat the bare seat and its `SEAT@TAG` as one holder, so rows written before this change still match.
- A git-hook claim takes its title from the commit subject when the seat's saved prompt belongs to another repository or an earlier sitting.
  A prompt recorded for the current sitting in that same repository still names the claim.

  A hook call finishes every open row that has been idle past the sitting window, one repository at a time.
  A fresh clock on the book, from a commit in a different repository, no longer keeps those rows open.
  A row written before it had its own clock is finished on that call when the hook is in another repository.
- A harness commit that names an open issue claims and cites that issue through the git hook's own booking function, instead of filing a new one.
- A lesson and any other bookkeeping memory tied to an issue now names that issue on the record.
  A later lesson for a different issue stays open beside it, and a revision of the same issue's lesson still replaces the old one.
- A policyd denial of `pack-unavailable: <text>` is the pack failing at runtime, the same unhealthy allow as the bare token `pack-unavailable`. The note keeps the pack's text. Named pack codes such as `shell-secret-in-argv`, `path-outside-workspace`, and `python-missing-pep723` still refuse. The command shims print that same note on stderr and match the unready tokens without regard to case or surrounding space.
- A policyd denial of `pack-unavailable` means the pack did not load. The command is allowed and the guard says it is unhealthy, the same as an empty reason, the bare token `phronesis`, and `bind-failed`. A denial that carries the pack's reason text still refuses.
- A policyd denial whose token is `phronesis:` plus words stays a refusal and keeps those words. Only an empty reason, the bare token `phronesis`, and `bind-failed` mean the seat is not ready; those allow the command and say the guard is unhealthy.
- A push is reviewed as a merge only when the commit it publishes is a merge. Sitting on a merge commit does not make a push of another branch wait on that merge's CI.
- A push with --force-with-lease or --force-if-includes is an ordinary push, the same line ljos-policyd draws, so the force-push reason names a command the gate allows.
  A guard reason's suggested command is one that same gate allows: one named volume can be removed, and clearing a stash points at listing, showing and applying a stash.
  A relative tool workdir resolves against the session directory, and a directory that cannot be resolved does not deny the command.
  An automatic session lesson is a sentence about the work and what the guard asked.
- A sitting that ends without a claim no longer leaves a book row whose issue is null. The prompt hook opens the row with a start sha and a title. Stop and SessionEnd now drop such a row, and so does the idle sweep once the row is idle. On willma-20261018d w5, session 1 timed out before any edit and left `book-u5cvnlu0hn.json` that way. A row with failures or asks stays, since its lesson and the failures it booked are the sitting's record.
- A test that needs a fake binary puts that directory in front of PATH while it holds the environment lock, and puts the previous PATH back when the guard drops.
- A tool hook no longer replaces the sitting's owner pid, so a short-lived hook does not make the sitting look abandoned.
- A tool hook that also carries a prompt field reads the command, so a commit is cited instead of being stored as the baseline.
- An MCP claim uses the tool's repo, or the last checkout the git hook booked for that seat, and refuses a directory that is not a git checkout.
  The tracker project is the one whose remote matches the checkout before a project is created from the directory name.
- An issue the hook files with no prompt to name it is titled after the project it files under, so a clone in `bench-w18b-tfn7` with origin `leidarljos/bench` opens "Work in bench".
- Codex PreToolUse does not honour `permissionDecision: ask`: the hook is marked failed and the tool runs. An ask there, and on Cursor `preToolUse`, is now a deny whose reason says how to grant it (`approve <tag>`, `ljos_request_approval`, or `ljos approve <tag>`). Claude Code, opencode, agy, and Cursor `beforeShellExecution` still receive `ask`. Gemini still receives `deny`, including when its event is `BeforeTool`.
- Recall orders an issue's memories by when each was written (`valid_from`), not by `ts`, which the writer restamps when it rewrites links or review clocks.
- The commit gate no longer ships a built-in person, and its denial names the config file rather than git's identity. A push is not held for test evidence unless `test` is configured. A policyd denial that names no check (the `phronesis` placeholder an overloaded seat prints when the decision text is empty) allows the command and says the guard is unhealthy. Hook and guard tests read an isolated config directory instead of `~/.config/ljos/user-rules.json`, and they do not sync user rules unless a test asks.
- The first prompt recalls a lesson while the search model is still loading, by ranking the atoms already stored, and a new session starts that load only after its opening line is ready.
  `export PATH="$PATH:..."` is no longer read as a script nested eight deep, so a shallow command beside it is not refused as nested-too-deep.
  A shell redirect that writes a file inside the work tree (`cat > f`, `echo > f`) claims on that call, the way Write does, including a file git does not track yet. A redirect outside the checkout does not.
- The first write or commit after `ljos_start` stays on the issue the start claimed. The MCP server keeps its book under the seat and the harness hook under the harness session, so the hook found no claim and filed a second issue: on the willma rerun, claude claimed work-z33v and then work-ddkw 6 seconds later, and opencode claimed work-rxm7 and then work-f3ko. A write or commit hook now takes the issue an `ljos_start` book holds for the same checkout, when the tracker has it STARTED under this holder as the bare seat or as `SEAT@TAG`. The row moves to the hook's book, so the server's shutdown does not finish it a second time without its deeds. A claim the seat's hold record names in this checkout's project, from `ljos claim`, is reused the same way. A row another hook session filed stays with that session.
- When the pack refuses the memory copy of `ljos note`, for a sentence past its word limit, the note stays on the ticket and the command prints a warning that names the logbook. The pack has no kind or flag that admits an agent's long sentence, so the ticket is where such a note lives.
- `ljos doctor` calls a runner env file that an older ljos wrote stale, not missing, and names `ljos onboard --harness NAME` to rewrite it.
- `ljos doctor` has a `git hooks` row. It says when `core.hooksPath` names a directory other than the hooks `ljos onboard` installs, or when stubs are missing there, and names `ljos git-hooks install` to point it back. An unset path is an optional part that is off.
- `ljos finish` without `--close` drops the claim and puts the issue back to the state it had before the claim, as `ljos release` does. It left the issue STARTED with nobody holding it, which hid it from every ready list. `ljos finish --close` and `ljos complete` keep the state.
- `ljos note` copies the note to memory as packset's `note` kind, which keeps three sentences or a long one, and sends it as a `summary` to a pack older than 0.15.2. ljos needs packset-core and packset-client 0.15.2.
- `ljos note` stores a progress note as a `summary`. Packset has no atom kind `note`, and a pack rejected that kind.
- `ljos recall` and the SessionStart and prompt hooks list an issue's own memories by their `issue:ID` tag, newest first, up to 20 with a count of the rest, then up to five related search hits not already listed. Before, recall showed the five best search hits, so newer notes on a busy issue were left out, and the hooks repeated the recall stored at the claim.
- `ljos release` puts an issue back to the state it had before the claim: the state the newest logbook change into STARTED came from, else TODO. A release used to drop the claim and keep STARTED, so the issue read as taken by nobody and left every ready list. `ljos complete` keeps the state as before.
- `ljos seat` and `ljos doctor` name the same seat. An exported `LJOS_SEAT` wins over a conversation id, and both say it came from `LJOS_SEAT`.
- `ljos_start` with no issue keeps the issue this seat already holds in the checkout instead of filing another. On willma-20261018d w1, the edit hook claimed work-ipph and `ljos_start` claimed work-8372 four seconds later. The held row moves to the server's book, and the next hook call takes it back. When the task differs from the title a hook filed the issue under, the task goes on the issue as a note, since vissue has no verb that renames an issue.
- mcp_forward now tries the spawn again for about 100 ms when the kernel answers ETXTBSY, as `tcb_answer` does. A server script written just before the forward, as in a test or right after an install, can be busy while another thread's fork still holds its write descriptor. The main push run on 2da1044 failed `a_forwarded_call_is_answered_by_the_fresh_server` that way.

### Documentation

- The docs site no longer loads sphinx-design, which no page used. custom.css states the insets of the header, the copy-page buttons and the edit link as plain lengths, with no change on screen. `.impeccable/config.json` records the detector ignores for the vendored theme stylesheet, command flags in code, and the house paper colour, each with its reason.
- The reference documents git hooks, user rules, and the commands it had left out.
  The doctor example uses the versions `ljos doctor` read from crates.io.
  Docs pages carry their titles, descriptions, and a preview image, and Pages deploys them from main and from release tags.
- The reference table shows a bar where its code cells printed `\vert`, and the rule row names `\b` and `\s` instead of doubled backslashes. The org export runs a pandoc filter that writes `\vert` as a bar inside code, so a later export keeps it.

### Miscellaneous

- A pull request into develop or main that touches the docs now exports the org sources, builds the site with Sphinx `-n -W`, and runs impeccable 4.5.2 on the built pages against `.impeccable/config.json`. Any finding fails the run. A pull request never publishes the site.


## 0.30.0 (2026-10-17)

### Features

- An agent's memory is written at once, with no person's approval first. `ljos remember` and the Model Context Protocol (MCP) `ljos_remember` under a runner, `ljos finish --lesson`, `ljos findings --remember` and the prompt hook's correction store a live claim with origin `agent-derived`; before, a finish lesson and a correction were proposals that only `ljos accept` from the person made live, and an agent's remember was an episode until a review. Against a pack that still keeps agent writes as proposals, the proposal is accepted at once. Secrets, the size cap and the two-sentence cap still refuse a write. Every write's `source` names the seat, the agent (`harness`), the session, the host, the time (`at`) and the reason: `LJOS_MEMORY_WHY`, or the opening of the person's latest prompt with key-shaped words redacted. `ljos memories list|review|tui|prune` reviews afterwards. `review` lists unreviewed agent memories newest first with their provenance and asks keep, edit, merge or drop; `--json`, `--keep ID`, `--drop ID`, `--edit ID --text T` and `--merge ID --merge ID [--text T]` script it. `tui` browses them in the terminal, filtered by seat (`s`) or date (`f`), and keeps (`a`), drops (`d`), edits (`e`) or merges marked rows (`m`, `M`). `prune --stale` names duplicates and memories never recalled in `--days` (30), and drops them only with `--apply`. Verdicts are kept in `$XDG_STATE_HOME/ljos/memory-reviews.jsonl`.
- `ljos onboard --harness aider` writes the skill as `~/.aider/ljos-conventions.md` and adds it to the `read:` list in `~/.aider.conf.yml`; the guard there is advisory. `ljos onboard --harness continue` also writes Claude-shaped hooks to `~/.continue/settings.json`, which cn's source reads but cn 1.5.47 does not yet fire. `ljos_file` names its type tag `issue_kind` in its schema and still accepts `kind`, since OpenHands refused to load the server over a field named `kind`.
- Agent hooks and ljos-mcp file a claim on the first edit or write, cite a deed when a git commit is seen, and finish at stop, session end, or server shutdown. The model does not have to call ljos.
- ljos-mcp takes each bot's name as `seat`, recalls and claims with `ljos_start`, checks a shell line with `ljos_guard_check`, and finishes on shutdown or when the session goes idle. Each tool result names the next step, and the server instructions are that loop.
- `ljos consensus` files a `Dissent holds OPTION` child, once, for a position a voter still keeps after the anchored settle, and says so; `--no-file` keeps it a read.
- `ljos consensus --quorum N [--deadline RFC3339]`: short of the quorum it says so and exits 1; past the deadline the tracker notes the lapse once and it exits 3. `scripts/suite-e2e.sh` walks three seats through vissues, claims, review, mail, a panel vote, a tie, a dissent, a lapsed quorum, deeds and finish, asserting from the tracker; `REAL=1` lets real model seats cast the panel ballots.
- `ljos finish ISSUE "lesson"` takes the lesson without `--lesson`, the way real agents write it.
- `install.sh` installs `packset-embed`, the encoder, by default. `--no-embed` leaves it out, `--with-embed` is still accepted, and a platform with no encoder build gets a warning instead of a failed install. On the 20-memory set the default install now recalls 20 of 20 paraphrases, against 7 of 20 without the encoder. The installer also fetches the encoder's model, so the first search is not lexical; a failed fetch is a warning.
- `ljos-mcp` now opens its server instructions with the session-start line, so a runner with no session hook still tells the model that ljos is on. goose, OpenHands, Continue, Roo Code, Kilo Code and Zed hear it this way.
- `ljos onboard --harness` now takes `auggie`, `amp`, `goose`, `openhands`, `continue`, `roo` and `kilo`. Auggie gets a `PreToolUse` hook on `launch-process`, Amp a delegate rule for `Bash` placed first in `amp.permissions`, and goose a user plugin whose `PreToolUse` hook fails closed, so the guard blocks in all three. OpenHands, Continue, Roo Code and Kilo Code get the server and the skill, and their guard is advisory, since none of them reads a user-level tool hook. A YAML snippet whose top-level key the config already has, such as goose's `extensions:`, now goes under that key instead of repeating it.
- OpenHands now gets a guard that blocks: `ljos onboard --harness openhands` writes `PreToolUse` on `terminal` and `UserPromptSubmit` to `~/.openhands/hooks.json`, which OpenHands reads at user level too. OpenHands hears the session-start line on a session's first prompt, since it drops a `SessionStart` hook's output.
- Ballots and design candidates are bound to the agent session and a per-process nonce, and the signature covers that binding. A second ballot with that session on the same question is refused at cast and ignored in the tally, and a session that filed a candidate cannot vote. A design whose session read another candidate before filing is flagged. `ljos panel run ISSUE` starts each designer and each persona in its own `cursor-agent -p` or `claude -p` session, and a designer is given only the brief.
- A prompt that names a tracker issue now gets that issue's recall from the prompt hook, so the working set reaches the model whether or not it runs `ljos recall`.
- `ljos file --quorum N --deadline RFC3339` records when a decision's ballots are due, and `ljos reap --apply` runs the quorum check on every vote past its deadline: one short of its quorum lapses once, noted on the tracker.
- A session's end keeps its facts: the SessionEnd hook reads the transcript and writes up to five durable sentences (rules, causes, fixes) as agent-derived lessons tagged `via:session-end` and the session, deduplicated by normalized form, with no model call. `ljos memories review` and `prune` clean them up; `LJOS_EXTRACT=off` turns it off.
- Shell runners with no pre-command hook (the Grok Bot box seats) get a command guard: `ljos onboard` writes rm, git and find shims that ask ljos-policyd first, and the seat env file puts them first on PATH.
- Seat ballots are signed: `ljos vote` signs each seat ballot with the seat's deedar host key and pins the key on the tracker on first use (`.ljos-seat-keys`). The push guard and `ljos consensus` count a seat ballot only when its newest signature covers that voter, issue and choice under the pinned key; unsigned, forged or altered ballots are named and refused. `LJOS_UNSIGNED_BALLOTS=allow` lets an older tracker through on purpose.
- The tracker is the record: `ljos vote` reads its ballot back from `vissue vote --json` and fails when the tracker does not hold it, printing `recorded on the tracker` only when it does, and persona briefs and agent definitions tell the model to confirm its row there and never report a ballot, claim or finish the tracker does not show.
- Tracker commits name the seat that did the work (`chore(issues): ID noted by saddle`), and the issue logbook gains a `Noted by saddle [timestamp]` line with no `:Seat:` property. `ljos log --seat saddle` and `ljos usage --since DATE` count those commits per seat.
- User rules sync through the tracker repository, one file per machine, and merge by id so the newest `updated_at` wins. A deletion is a tombstone, and a hook that cannot reach the tracker still shows the rules on this machine.
- Standing user rules, written with `ljos prefer --user` and `ljos rule --user`, are shared by every seat and stay in force without a review date. The brief, the session start and the prompt hook lead with them, and the guard denies a `gh` or GitHub comment that a rule's mention or reply fields name.
- Validity windows: `ljos remember --valid-from DATE --valid-to DATE` records when a claim held in the world, beside the pack's own clock. `ljos search` shows what is true today, `--valid-at DATE` what held on another day, and `--valid-at any` every window.

### Fixes

- The `admit_pack` test takes a writer that stores an agent's write live (packset 3aec57f and later) as well as one that files it as a proposal.
- An agy sitting keeps the prompt from its payload, so a lesson written after a timeout still carries those words and the next session can recall it.
- Antigravity (`agy`) now hears the "ljos is on" session-start line on the first model call of a conversation, as the other runners do.
- Bind command consent to the full conversation identifier while keeping shortened names for claims. Relay consent forms through the installed server when its executable is replaced. Accept a registered seat on consent calls while requiring the client confirmation form to grant approval. Show short approval tags in chat and terminal prompts, with exact matching and collision checks. Read confirmation replies from their answer field; quoted questions and negated approval text cannot grant consent.
- The Cursor hooks `ljos onboard --harness cursor` writes now include `sessionStart`, so the Cursor CLI model hears the "ljos is on" line.
- A prompt naming an issue with nothing remembered on it no longer gets an empty "Recall for" block.
- `ljos finish` without `--lesson` no longer says no lesson was remembered: it names the missing flag and that an earlier `ljos remember` is still in place.
- `ljos finish --lesson` keeps a lesson of three or more sentences as separate claims of at most two sentences, in order. The pack refuses an atom of more than two sentences, so such a lesson used to be turned away whole.
- The MCP seat follows the hook seat, from `LJOS_SEAT`, the config file, and the agent's id, and an agent name does not replace a configured seat. An issue passed as `none` is the one this session claimed, and the session-start line names that id. An agy stop finishes the claim when the turn is idle, and a mid-loop stop leaves it open. A tracker with no seats list gains the agent's seat and logs a note, instead of refusing the start. A commit the guard refused is not cited, and a force-push check reports verdict ask.
- A Grok Bot env file from `ljos onboard --harness grokbot` no longer overwrites a seat already exported, so each bot on a shared box keeps claiming under its own seat.
- A force push, a `+refspec`, deleting a remote branch, and pushing tags ask by default, with or without a seat rule. A plain branch push stays allowed. A tracker checkout writes `.gitattributes` so org files union and atom logs re-export on merge, on init, onboard, and every sync. When a rebase stops on a conflict the push merges instead, then folds duplicate `:ID:` headings. Nothing is force-pushed.
- The guard waits 5 seconds by default, and `LJOS_GUARD_TIMEOUT` changes that. A timeout or a crash allows a command no seat rule would refuse, and says the guard is unhealthy. A force push, a seat deny rule, and an `rm -rf` outside /tmp are judged in this process before that wait.
- An ordinary `git push` of a branch is allowed, including to main and when the remote's owner cannot be read. Force, deleting a remote branch, and pushing tags can still ask. A pattern as wide as `git push*` is refused when a rule is written, and an agent cannot file a pattern broader than the command it cites. `ljos approve` works from the person's terminal, and the approval tool grants a request that matches the seat, the directory and the request id. A tracker push with no upstream uses `git push -u origin <branch>`. When another bot pushed first, the push fetches, rebases with autostash and retries; an org conflict keeps both headings, keyed by `:ID:`. Nothing is force-pushed, and a failed push is reported as failed.
- A lesson keeps the prompt's distinctive words, names the guard rule and the failing command, and keeps the full title in the atom source when the prose is shortened. A refusal from the pack names the HTTP status and the response body. agy registers an after-command hook as PostToolUse, and a payload with no event name is that return when it carries the command's error, so the commit is cited. An opencode commit is cited from the command the before hook saw, including when git exits 0 and its stderr is on the result. A sitting whose runner has exited is finished at the next session, so a timeout does not leave the claim open. A deed whose commit is no longer in `HEAD` is recorded on the commit that landed, and the lesson names that one.
- A commit deed is cited from the post-tool hook after exit 0, with accession `deed-file-commit-<sha>` (a file deed's id has to start with `deed-file-`) and the sha as its body, on the ticket claimed for this repository. The first Write, Edit, MultiEdit, apply_patch, or shell redirect onto a tracked file claims. Stop and idle write one agent-derived lesson, and the first prompt recalls it lexically while the embedder loads. `git-force-push` asks, including on a runner whose stdin carries `turn_id`. A ballot's session is the agent's id, `VISSUE_AGENT`, or the MCP connection, so `--as` personas from one holder count as one vote.
- `ljos recall` now shows what the seat remembers about the issue, searched on its id and title, under a `Memory` heading after the tracker's working set. It used to print only the tracker's part, so a seeded lesson never came back on recall in any agent. Codex and opencode now hear the session-start line. `ljos onboard --harness codex` registers `SessionStart` alongside `UserPromptSubmit` and `PreToolUse`, and the opencode plugin carries the start line on a session's first prompt. Found by the real-model test rig, where both showed no `ljos is on` while Claude Code did.
- A note and a finish lesson written through `ljos` or ljos-mcp are stored as agent-derived memories in the pack the box's `ljos memories list` reads, and a recall bumps the use count that list prints. The accept step used to relabel the atom user-declared, which the default list hides, and nothing incremented `recalled`.
- A second ballot with the same session on one question is refused at cast and left out of the tally, including five personas cast from one sand subagent and a later recast of those same personas. The check used to run only when `ljos consensus` settled, and vissue's `:VOTES:` drawer counted each persona. A session that filed a candidate cannot vote. The model settle reads only the counted ballots.
- The opencode plugin's after hook reads the tool arguments from the input, so an edit is not reported as a failure when the seat records it.
- A panel that has settled records a nit, a wording note or a style note on the issue and files a child only for a blocking objection with a reason. The same objection is not filed again, a dissent child files no grandchild unless the new objection is blocking, and at the round limit (three by default) or when the panel or workflow budget is spent the item closes as settled-with-notes. `ljos reap` flags a tree that has run away.
- Recall and a new session start the memory store when it does not answer, as the doctor does, instead of telling the agent to run `packset ensure`.
- The release workflow parses again, so a version tag builds and attaches the ljos archives and install.sh; CI now rejects a workflow file with a repeated key.
- The `rm -rf` denial no longer suggests moving the directory under /tmp, which real models used to delete it anyway; it offers the trash or the person.
- The guard-shim, force-push, and harness-hook tests create the home and working directory they use. Those directories sit beside the crate, mode `0o755`, because a clean runner has no `/workspace` and cannot create one.
- The command guard and the pack daemon stay quiet when `HOME` or the working directory is a temporary sandbox, and when LJOS_OFF=1. A sync with no remote says none is configured and how to add one, instead of pushing to nothing.
- Each seat signs with its own key, made on its first vote at `$XDG_DATA_HOME/ljos/seats/SEAT/ballot.key` (or `LJOS_SEAT_KEY`), so seats sharing one `HOME` sign apart with no setup. A ballot the tracker committed before signing began counts as legacy unsigned and is named so; only new ballots need a signature. `ljos search` asks the pack for more hits when validity filtering will drop some, so the limit applies after expired claims are gone.
- ljos-mcp keeps the open claim on disk under the seat, so a server spawned once per tool call still claims on the first write and finishes that sitting when a later start finds it idle. The book used to be keyed by the process id, and shutdown of a one-call process never closed it.
- `ljos sync` pushes commits already on the tracker even when the atom files are unchanged. It used to commit a pending change such as `.gitattributes` and then skip the push.
- Finish's sync skips the seal when `age` is not installed, with one line saying so, instead of printing an os error.
- Tests that read or write the process environment take one lock, and each one is given a private runtime directory that is put back when the test ends. Seen files and hook hold markers live in that runtime directory, so a cleared `XDG_RUNTIME_DIR` cannot land them in a shared `/tmp/ljos`. Scratch directories and sockets those tests create are private to the test instead of a path named only by the process id. The guard a seat test takes sets aside a seat name, a hold, and a runner session the process was started with, and puts them back afterwards.
- A unanimous settle no longer reads "no option leads: the settle is a tie"; it names the option every voter is on.
- A deed is cited only after the commit command ran and `HEAD` moved past where the sitting started. A sitting that never claimed still leaves a lesson when a tool failed or the guard asked.

### Documentation

- The README's plugin sections point at the `install.sh` line instead of `cargo binstall`, which the quickstart no longer uses.

### Miscellaneous

- Clippy with `-D warnings` passes: ballot helpers elide the unused lifetime, a refused tracker push records the error with `inspect_err`, and seat-hook checks pass the predicate directly. The policyd allow guard is one negation, a test script with no placeholders is a string, and one-rule slices borrow the rule. The hook test reaps the pack server after stopping it.
- A push to develop or main that passes the tests uploads a bookworm build of that commit, kept for fourteen days. The archive is `ljos-<sha>-x86_64-linux.tar.gz` with its sha256, and contains ljos, ljos-mcp, ljos-hud, and the vissue, packsetd, and ljos-consensus binaries from the seat bundle.
- The Cursor plugin manifest, the Claude Code plugin manifest and the marketplace entry name 0.30.0 and give leidarljos as author and owner, with no email address.

## 0.29.0 (2026-10-13)

### Features

- `ljos send` wakes a recipient that is idle, and a running session reads interrupt mail before it stops. Mail waited for the recipient's next prompt, so a Claude Code, Codex or Cursor session that had gone quiet never saw what a Grok Bot sent it. `ljos wake --command CMD` sets a shell line that runs with `LJOS_SEAT` set to the seat, such as `claude -p --resume` or `codex exec resume`, and `ljos wake --webhook URL --key-file FILE` posts the envelope, never the body, to a Grok Bot routine's webhook. `ljos send` and `ljos reply` start that wake in its own session after the message is written, so the sender neither waits nor fails on it, and the guard judges a wake command when it is set and on every run. The `Stop` hook holds a turn once for unread interrupt mail, as `decision: block` or Cursor's `followup_message`, and receipts it. A seat the pack has never heard from is refused with the seat it most likely meant, so `ljos send ljos` asks whether you meant `ljos-bot`. ([#95](https://github.com/leidarljos/ljos/pull/95))
- `ljos mcp` serves MCP over stdio by running the `ljos-mcp` installed beside `ljos`, so a client that starts a server by its crate's name reaches it. `tools/list` now gives the core set in a fixed order with the command rules, the consent form and memory first, then the other tools by name, then the persona and playbook tools; it was alphabetical, so `ljos_brief` led the full list and `ljos_claim` the core one. The repository carries the listing files: `server.json` for the official MCP Registry, the `mcp-name` line it checks in the README on crates.io, an `mcp-registry` job in the publish workflow that, once a release tag's crate is on crates.io, publishes the entry with the workflow's OIDC token, `glama.json`, and a `Dockerfile` that builds `ljos-mcp` and starts it. ([#96](https://github.com/leidarljos/ljos/pull/96))
- `ljos persona NAME --retire` withdraws a persona, so it leaves the roster and casts no more ballots. Only the conversation that defined it may retire it; `--replace` retires one another seat or conversation defined, as it takes over a name on a define. Before, a persona could only be overwritten, never removed. `ljos persona NAME --view TEXT --check` says what the define would do and writes nothing: whether the name is free, this seat's, or another seat's and so refused, and which live personas already hold the same view or speak to the same domains. Two agents on one seat built overlapping panels, such as `rust-perf` and `rust-idiom`, and the check names the overlap before the second is written. ([#98](https://github.com/leidarljos/ljos/pull/98))
- Feature work runs one loop the seat remembers: design, implement and review each child, then a panel vote. `ljos workflow PARENT --start feature` notes `workflow: feature` on the parent, tags it `workflow-feature`, and files a `design:` child; run again, it files nothing new. `ljos workflow PARENT` prints the round, the stage and the next step, read from the parent's children by their `stage-*` tag or their title (`design:`, `impl:`, `fix:`, `review:`, `panel rN`). `ljos workflow PARENT --round` files the next `panel rN` decision once every child is closed. `ljos finish PARENT --close` is refused, with nothing written, until the latest round passed and no child is open. A round passes when every panel persona has cast, no veto persona voted `revise`, and `pass` holds a share of at least 0.6 that is not a tie. The panel is tester, senior-engineer, junior-dev, ceo and cto, with the tester's veto, read through one settings function that returns these defaults. There is no round cap, and nothing in the loop waits on the person: a round that does not pass names its reasons and the fix children to file. A sitting on the parent binds the new `feature` playbook, and a sitting on a child prints the parent's status under `== workflow`. The workflow outlives every sitting: finish and release drop the playbook note, not the workflow. A title with the word "feature" still binds `sit`. ([#99](https://github.com/leidarljos/ljos/pull/99))
- `ljos reap --idle 6h` lists claims with no claim or tracker commit inside the window; `--apply` releases each one and mails its seat how to pick it up or finish it.
  `ljos recall`, `ljos search` and `ljos hook` append one line to `recall.log` in the ljos state dir, so an audit can tell when a seat last asked its memory.

### Fixes

- `ljos search` prints its hits alone. Every result list opened with three sentences on what the score and the fraction mean, so a person read the same note above each answer before the hit they asked for. `ljos search --explain` opens with that note, and an empty answer is the one line `No hits.` The hit lines and their columns are unchanged, and the MCP tool still returns the note beside its hits. ([#98](https://github.com/leidarljos/ljos/pull/98))
- One ordinary word of a longer prompt no longer brings a memory in. It used to, whenever the cosine sat within 0.05 of the prompt's best, and unrelated prompts then received memories that share `test`, `commit` or `build`. Below the encoder's confidence line a memory is handed over when an alias names it, when two content words are shared, or when the prompt has two content words or fewer, so the overlap is at least half of what it says. The encoder's first hit at a cosine of 0.66 or more still comes in with no shared word, whether the lesson is standing or an episode. ([#100](https://github.com/leidarljos/ljos/pull/100))
- `ljos search --rerank` keeps the claim that the fused ranking and the encoder both placed first, and the cross-encoder reorders the claims under it. The second stage had been free to push that agreed winner out of the top of the list. A leader the encoder did not rank first is still reordered. ([#100](https://github.com/leidarljos/ljos/pull/100))
- A lesson written with `ljos remember` from an agent runner reaches the prompt when the gate is sure it bears on it. Before, that write was an episode and the hook hid every one of them until a review, so the ten lessons in a twenty-memory pack never arrived even when `ljos search` ranked them first. A preference and a lesson the person typed are unchanged. A lesson with no horizon tag still waits for rehearsal. ([#100](https://github.com/leidarljos/ljos/pull/100))
- A prompt that names a concept reaches the preference or lesson about it. "add the lodash dependency to the web app" never brought back "Use pnpm instead of npm for every JavaScript package install", and "can I use MERGE ... RETURNING in this query" never brought back "Our staging Postgres is version 15". Neither pair shares a word, and the encoder put both under its 0.66 confidence line. Two alias groups now count as one word each: a dependency, package, library or install, and a query, SQL or a named database engine. Such a memory still has to clear the score floor and sit within 0.05 of the prompt's best cosine. `postgres` is looked up as written, where it had been read as the plural of `postgre`.
- A seat named from the process tree is a short program name. On Arch, `ljos doctor` under the Grok Bot desktop app printed a seat hundreds of characters long, starting `grok-bot-standard-schemes-sand-media`. Electron rewrites its command line into one word with every flag in it. The name now ends at the first flag and keeps 32 characters at most, cut at a hyphen. An agent runner in the chain wins over a desktop app: Claude, Codex, opencode, agy, Grok, or one the runners file names. `LJOS_SEAT` and `LJOS_HOLD` still come first.
- Every call ljos makes to packsetd sends the bearer token from the pack home, so memory, mail and recall work against packsetd 0.15, which answered each of them with a 401.
  The token is `PACKSET_TOKEN`, else the `token` file named by `PACKSET_TOKEN_FILE`, else `token` under `PACKSET_HOME` or the default pack home, as packset 0.15 resolves it.
  `ljos doctor` prints a `pack token` row that fails, naming the file it looked for, when the writer refuses the seat.
- The prompt hook hands back a package-manager rule on a prompt about packages. Packset 0.14 sends no encoder cosine, so the hook needs a shared word or alias. "What package manager do we use" printed nothing beside a pnpm preference that all three scorers ranked first. Now `package`, `dependency` and `lockfile` share one group with npm, pnpm, yarn, bun, pip, uv, pipx and poetry. A word can sit in two groups, so pnpm still names JavaScript. Rust rules name `cargo` to build or test, so `cargo` stays out.
- `ljos complete` and `ljos finish` finish a claim taken under a conversation or host-suffixed holder when `--assignee` or the seat names the same bare seat the tracker or hold record gives.
  `ljos claims --agent NAME` now passes `--by NAME` to vissue, which rejected `--agent`.


## Unreleased

- `ljos doctor` prints how many of the workspace's preferences a rule enforces, such as `6 of 10 preferences enforced by a rule`. A preference counts when a deny or ask rule carries it as its reason, as `ljos prefer --deny` and `--ask` write. The row also says how many more name a command they forbid, and the command that turns one into an ask.
- `scripts/install.sh --version VER` installs that ljos release, and `--pin REPO=VER` installs another repository at a version; with neither, each repository is at its latest release, as before. A pinned version with no release for the platform builds that version's crate with `cargo install --version`. When the gh CLI is signed in, the script checks each tarball's GitHub build attestation and says whether it verified, and `--attest` stops the install when one does not. The release workflow attests each archive and `install.sh`.
- `ljos doctor` starts the default pack writer when none answers, as `ljos remember` does. Right after `install.sh` on a fresh machine, `ljos doctor` said `not ready: pack does not answer`, because nothing had started `packsetd` yet; the first memory verb started it and wrote the token. A seat whose `PACKSET_URL` names another writer, or is `off`, is left alone.
- Each command that runs past the guard is logged to `$XDG_DATA_HOME/ljos/bypass.jsonl`: a one-use approval that let an asked command run, and a line the ask pack would have asked about that ran because `LJOS_ASK_PACK` left its class out. Each row says when, how, the command, the rule it passed and the seat. `ljos doctor` prints a `guard bypass` row with the count, the last command and the file.
- A permission hook that fails on Cursor is a deny in the CLI too. Cursor's IDE reads a `failClosed` hook that prints nothing as a deny, and Cursor's CLI reads the same silence as an allow, so a seat hook that hit an error or a panic on `beforeShellExecution`, `preToolUse`, `beforeMCPExecution` or `beforeReadFile` let the CLI run the call. The hook now writes a deny in the runner's JSON and exits 2 when it fails on a tool call, on every runner, and on Cursor it refuses a call it has no verdict for after 8 seconds, inside the 10 seconds onboard registers. A prompt or session hook that fails lets the work go on as before. `ljos onboard --harness cursor` adds `failClosed` to a seat hook an earlier onboard wrote without it, and `ljos doctor` counts that hook as missing until then.
- The ask pack asks before a `git stash drop`, a redirect that empties a file (`: > src/main.rs`, `> file`, `true > file`, `cat /dev/null > file`), an `rm` or `unlink` of a home dotfile such as `~/.bashrc` or a file in `/etc`, an `rsync --delete` into the home directory, and a `PATH` that puts `/tmp`, `/var/tmp` or `/dev/shm` ahead of `$PATH`. Each ran without a word. `git stash`, `git stash pop`, `: >> log`, `echo hi > out.txt`, a delete under `~/.cache`, and `PATH=./node_modules/.bin:$PATH` still run. The stash ask is class `git` and the PATH ask is class `path`, so `LJOS_ASK_PACK` can leave either out. On the 209 destructive lines of the guard corpus, 5 more now ask, with no new ask among its 204 harmless lines or the 85 daily ones.
- A preference reaches the agent when it acts. Before, it came back only on a prompt that shared its words. With ten preferences stored, `npm install lodash`, `pip install black`, `git push origin main`, `echo STRIPE_KEY=... >> .env` and `psql -h staging-db -f migrations/007.sql` all ran with nothing said. The tool-call hook now hands over, once a session, the preferences that name the command's program, its subcommand or a file it writes to, read from the pack listing it already makes for the rules. A preference that forbids a command it can name, such as "Use pnpm instead of npm", carries an ask rule over that command. Typed by the person, `ljos prefer` writes the rule and prints the `ljos forget` line that takes it back; under an agent it prints the `ljos rule` command instead. Five of the ten preferences in the test set carry a rule.
- The hook reads what one command on a line hands the next. `x='push -f'; git $x origin main`, `` `which git` push -f ``, `cp /bin/rm /tmp/del && /tmp/del -rf ~/projects`, `ln -s /usr/bin/git /tmp/g && /tmp/g push -f`, `IFS=,; cmd=rm,-rf,/home/u/proj; $cmd`, and a script written with `echo`, `printf`, `cat <<EOF` or `tee` and then run with `sh`, `bash`, `.` or its own path all ran, since each command was read on its own. A variable a bare or exported assignment set now stands in for `$NAME`, split on `IFS` when the line sets it; a program copied or linked to a new path is read as that program when the line runs the copy; a command word found with `which`, `command -v` or `type -P` is read as the command it names; and a script the line writes and runs is read as its lines. On the 209 destructive lines of the guard corpus, 7 more are refused, with no new refusal among its 204 harmless lines or the 85 daily ones.
- A write the pack keeps over a prose warning prints one line on stderr: what the pack's prose check said, and that shorter sentences are recalled better. With packset's change that stores a person's hard sentence as written, `ljos remember` of "The payments service retries webhooks three times with exponential backoff starting at 2 seconds" is kept and says so, where packset 0.14 refused it at readability grade 14.78. The line a script reads on stdout is unchanged.
- `ljos consensus` refuses an issue with no ballots, before any settle runs, and the MCP tool refuses the same way. The message says how to cast a ballot. It ran the settle anyway, and the model answered `settled: true` with no options and a margin of 0, which a caller read as agreement. `ljos consensus --json` prints one JSON document and nothing else on stdout. The push cite stays at the top level under `push_cite`, with the keys it already used. The model crate's settle is under `settle`. When `--compare` ran, the tracker's settle is under `tracker`. `surprising` and `reputation` are present when those readings ran. It printed the push cite's JSON and then more JSON and plain text on the same stdout, so `json.load` failed with `Extra data`.
- A standing choice said in chat is saved at once, in the person's words. The prompt "From now on always use bun, never yarn, in this repo" stored nothing, and an agent's `ljos remember` of the person's words was an episode the hook hid until a review, so lessons written from a runner reached the prompt 0 times in 20. The prompt hook now saves a sentence that opens with "from now on", "always", "never" or "remember that", outside code and quotes, as a standing preference with origin `user-declared`, and says so in one line with the `ljos forget` command that takes it back. Questions, pasted output and quoted rules save nothing. `LJOS_CAPTURE=nudge` asks the agent to write it instead, and `off` turns it off. An agent's `ljos remember` whose words are in the person's latest prompt stands at once.
- The prompt hook hands over a memory the pack's encoder ranks first, even when it shares no word with the prompt. It needed a shared word and two scorers in agreement, so "add the lodash dependency to the web app" never brought back "Use pnpm instead of npm", and 5 of 20 paraphrased prompts lost their memory for want of a shared word. With a pack that sends each hit's encoder rank and cosine, a claim reaches the prompt when the encoder ranks it first at a cosine of 0.66 or more, or when it shares a word with the prompt and sits within 0.05 of the prompt's best cosine. The cross-encoder is not asked then, and the hook answered in about 85 ms instead of about 700 ms. A word counts through its other names, such as golang for Go and virtualenv for uv. A prompt gets the three strongest memories at most, preferences read first. With a pack that sends no cosine, such as packset 0.14, the hook keeps the two-scorer check.
- The prompt hook saves a stated choice, grants an approval or files a correction only from the person's own prompt. Codex sends a prompt event inside a subagent with the parent agent's brief as the prompt, so a brief stating "Always run the tests against the staging database" was saved as the person's preference, and a brief with `approve ID` in it granted the agent's own request. A payload that names a subagent with `agent_id`, as Codex and Claude Code send inside one, or a Codex rollout file for a thread other than the session's, is now an agent's words, and so is every prompt while `LJOS_UNATTENDED=1` is set. The memories the prompt bears on still reach it. Codex sends each hook event twice, and its copy of a prompt or tool result got no answer; it now prints the first delivery's answer, still without saving twice.
- `ljos vote` and `ljos predict` refuse to cast when nothing named the seat. With no `LJOS_SEAT`, a seat fell back to the program above the shell, and a subagent on the box cast 25 forecasts as `exec-daemon`, the command daemon, with no warning. `LJOS_SEAT`, `VISSUE_AGENT`, an MCP client's name or a runner's record still name the seat, and a person at a terminal is still their login user. The refusal says to set `LJOS_SEAT`, and nothing is written.
- `ljos send SEAT` refuses a seat the pack has never heard from, and names the seats it knows. Mail waits under the name it was sent to, so two messages to `ljos` sat unread while the seat that meant to read them was `ljos-bot`. A seat is known once it has written anything to the pack, read a message, or joined a group. `--new-seat`, or `new_seat` on the `ljos_send` tool, writes to a seat that has not started yet.
- `ljos release`, `ljos complete` and `ljos finish` drop the tracker's claim along with the claim graph's. The claim graph freed the node and the tracker kept `CLAIMED_BY`, so `ljos claims` listed every finished sitting as still held: 153 claims on one box, 70 of them done. The issue keeps its state, so a finish without `--close` leaves the work open and unclaimed. A claim another name took since is left alone, and the command says so.
- The hook asks the person before a command that drops or resets a database, tears down infrastructure, deletes cluster or cloud resources, prunes containers or volumes, publishes or yanks a package, prints a secret, or empties a file. Nothing caught these unless the person had written a seat rule for each, so `terraform destroy`, `prisma migrate reset --force`, `docker system prune -af --volumes`, `npm publish` and `cat ~/.aws/credentials` ran without a word. Each ask names its class and gives a reason and a safer form, such as `infra-destroy: ... Show the person the plan first (\`terraform plan -destroy\`)`. Routine work on the same tools still runs: `kubectl delete pod NAME`, `docker compose down`, `npm publish --dry-run`, a `SELECT` through `psql`, and `cat .env.example`. A seat rule on the same command comes first. `LJOS_ASK_PACK=off` turns the pack off, and `LJOS_ASK_PACK=db,cloud` keeps only the classes listed.
- `ljos deed --remove` works on an issue that is still on a board. It refused with `not in a ledger`, so an issue filed with `ljos file` minutes earlier kept a superseded deed cited. The removal now splits the board into per-issue files first, as `vissue migrate-ledger` does, and then appends the removal. The board file is read and not written. `ljos deed ISSUE --remove ACCESSION` takes the issue as the positional argument too; it failed with a usage error and needed `--ticket`.
- `ljos brief` and `ljos panel` hand a persona ljos's own principles and arena rubric only on ljos's own issues: an `ljos-` id, a tracker project named `ljos`, or an `ljos` tag. A brief on a numerical-methods ballot or a logo review printed the eight rubric items about plugins and pack atoms, and a persona starting from it reasoned from them. Another project's brief keeps its playbook and says no product rubric applies.
- A persona names the seat and holder that defined it, and when. Personas are shared by every seat on a pack, and `ljos persona NAME` on an existing name replaced its view and anchor without a word, so two agents on one seat built overlapping panels within minutes. Redefining a name another seat, or another conversation of the same seat, defined is now refused with who defined it; `--replace` takes the name over. A settle that moves an anchor keeps the owner. `ljos personas` shows the owner and the date. A persona written before this has no owner and can be redefined as before.
- A name passed to `--assignee` on the command line holds the claim, the seat's own name too. `ljos sitting ISSUE --assignee saddle` from seat `saddle` claimed under the conversation's holder, a session id, so the tracker's `CLAIMED_BY` named that id and a board filtered by seat missed the claim. `sitting`, `finish`, `claim`, `complete` and `release` now take the name as given; with no `--assignee`, or `seat`, `you` or `agent`, the holder is the conversation's as before. A tool call's `assignee` field still reads the seat's own name as the conversation, since runners echo it.
- `ljos finish` completes the session node before it files the lesson. When the pack refused the `--lesson` text, for example on its readability grade, the whole finish stopped with exit 1 and the node stayed open, though the work was done. Now the node completes, the refusal is a line with the pack's reason and the `ljos remember` command that keeps the lesson once reworded, and finish exits non-zero only when the completion itself fails. A pack that does not answer the island no longer fails the finish either.
- `ljos evidence` prints the time a deed was produced as an `at=` line in ISO 8601 with this machine's UTC offset (`at=2026-10-10T16:24:30-05:00`), after deedar's `time=` line in seconds since the epoch. The `time=` line is unchanged, so the timeline and scripts that read it keep working.

- `ljos send` and `ljos reply` carry a message longer than 500 characters. The pack refuses an atom whose text passes 500, so a long message bounced with `atom text exceeds soft cap 500`. The atom's text now carries the envelope and the start of the body, cut with an ellipsis, and the whole body rides in the atom's `body` field, which `ljos inbox` shows. The prompt hook still shows the first 160 characters, and now says how long the body is and that `ljos inbox --all` shows it. A body past 16,000 characters is refused before anything is sent, with the `deedar create file` line that sends it as a deed.
- `ljos finish --outcome` and `ljos learn --outcome` take `--new-option` for an answer that was on no ballot, such as a tie the person broke with a third answer. Every voter is scored a miss on it, the outcome is kept, and the reading says so. Without the flag, an outcome that matches no ballot and none of the issue's options is refused with the options it named, because a typo would score every voter a miss too and nothing said so. An outcome that matches an option in another case takes the option's spelling. The MCP finish and learn tools take `new_option`.
- `ljos sitting` and `ljos finish` say nothing about sync when the tracker repository has no `.ljos/sync.toml`, and the sitting leaves out its `== sync` section. Every sitting printed `names no scope; write .ljos/sync.toml`. Sync is opt-in, and on a machine that never set it up the line nudged agents toward a scope file and a remote nobody had asked for. `ljos sync` run by hand still names the missing file.
- `ljos file -p PROJECT` files at the top of PROJECT when the held issue is in another project, and says so on stderr with the `--parent` that files under the held issue. It made a child of the held issue, so a bug in ljos found while sitting on another project's issue needed `--top -p` to land in the right place, and no help text said so. `--parent` and `--top` work as before. The skills, the protocol and the MCP tool say how to file a bug in a tool mid-sitting.
- `ljos doctor` prints `warn` on the host row after an OOM kill within a day. It printed `no` and still exited 0, and `ljos sitting` opened under it, while the skill says not to proceed on a `no`. The word now matches what the doctor and the sitting do, and the sitting adds a line under its doctor: the kernel killed a process within a day, so run one build or test suite at a time.
- `ljos note` on an issue another seat sits on names that seat and says the note is an assist the holder reads in the logbook, with `ljos send SEAT` to reach the holder now. A sitting refused on such an issue names the same two ways to help. Before, the sitting was refused and the note went in without a word, so the two verbs seemed to disagree.
- A new session hears one line from the seat before the first prompt: `ljos is on: 42 memories, 3 due for review, command guard on.`, then up to three of the workspace's preferences. A memory store that does not answer, or a missing `ljos-policyd`, is named in that line with its fix. A session resumed after a compaction still hears its held issue instead.
- A lesson the person types at `ljos finish` in a terminal of their own, with no agent runner above it, is written at once as theirs. Before, it was filed as agent-derived and waited for `ljos accept`. A lesson an agent gives is still a proposal.
- `ljos accept`, `ljos forget` and `ljos graded` take a short id, such as the first eight characters, when it starts exactly one open proposal, memory or due row; one that starts several is refused with the ids it matched. At a terminal, `ljos remember` and `ljos prefer` print `remembered (preference): TEXT Review 2026-10-12 09:00 -05:00. id 922d35cd`, and `ljos graded` prints the next review in local time. A pipe gets the same tab-separated row as before.
- A bare `ljos onboard` at a terminal finds every coding tool on the machine and onboards each, by its command on the PATH or the directory its config sits in. `--harness all` does the same from a script. Into a pipe, a bare `ljos onboard` still prints the MCP server entry, and `--harness json` always does. Cursor with the ljos plugin installed is left to the plugin, as `--harness cursor` already did. A runner that fails is said and the rest go on.
- The README and the tutorial state the minimum Rust for a source build, 1.89. `ljos doctor` marks an `ljos-policyd` older than 0.3.0 `no` and exits 1, naming `rustup update stable` and the reinstall. Before, a Rust 1.85 install left `ljos-policyd` 0.2.6 in place, and the doctor's `ljos-policyd` row said `ok`.
- `ljos consensus` names the standing of the issue's voters only. The EigenTrust step runs over every trust row about the topic, and its JSON beside the settle listed personas that never voted, at 0.0807 each on one issue, as if they had weight there. The others are now counted on one line, and `--json` and the `ljos_consensus` tool still return the full JSON.
- A person typing in a terminal window is their login user, as the protocol says. The seat came from the first program above the shell, and in a terminal window that is the terminal, so a person's sitting held as `gnome-terminal-server-17j3f`, and their memories read `from: gnome-terminal-server`. On a terminal that runs one process per window, a finish typed in a second window was refused as held by another. Twenty-nine terminal emulators now end the process tree the way tmux and sshd do. A runner started inside a terminal, such as Claude Code or Codex, sits below it and is still the seat.
- A conversation holds its claims as its seat and a short tag, such as `saddle@k3j9x0a1b2`. The holder was the conversation id the runner stamped, as it came, so a board listed claims under names like `4eeb651b-18e3-4c42-910e-5e16f4f7825b` and nobody could tell whose they were. The tag is the hash the seat already files its session records under, so two conversations of one seat still hold apart. A conversation that took a claim before this change keeps its old holder through its session record. `LJOS_SESSION_ID` alone, the name a person or an env file chose, is kept as written.
- A refusal from `ljos-policyd` says what the command would have done and what to run instead. The hook passed on the check's token alone, so a runner showed ``git-force-push (seat rule `ljos-policyd`)`` and the agent had to guess the next step. The token still comes first, so logs and scripts that look for it keep working. A token this build does not know is passed through as before. A missing `ljos-policyd` under `POLICYD_REQUIRED` names the install line.
- `ljos doctor` opens with one line that says whether ljos is ready, then prints only the rows that need action, each with its fix. `ljos doctor --all` prints every row, as before. `ljos sitting` prints a short report: that line, what needs action, what the seat remembers for the task, and the claim. `ljos sitting --verbose` prints every section, as before. The tool server's sitting is unchanged.
- `ljos complete`, `ljos finish` and `ljos release` act on a claim that another conversation of the same seat took: a background worker the conversation started, or the conversation that started the worker. Before, a seat could not complete or close the claims its finished workers left, and a worker could not finish its parent's claim; both failed with `not assignee`. The output names that conversation. A claim of another seat is still refused.
- `ljos prefer TEXT --deny PATTERN` writes the preference and a deny rule over `PATTERN` with the preference as its reason; `--ask PATTERN` writes an ask rule. A preference alone is recalled on a prompt that shares its words and is never enforced, so "never run terraform destroy" was remembered and `terraform destroy` still ran. `ljos rules` lists the rules the hook judges by, deny first, with `--json` for a script.
- The hook keeps the seat rules it last read from the pack in `$XDG_DATA_HOME/ljos/rules-<workspace>.json` and judges by them when the pack is down, slow or switched off. Before, a pack that did not answer meant no rules, so a deny written with `ljos rule` stopped nothing and the command ran without a word. `ljos policy` reads the same rules.
- An `ljos-policyd` that is installed but fails now refuses the line: a crash, an exit with no verdict, output that is neither `allow` nor `deny`, or no answer in five seconds. Before, all of these counted as allow, and a hang held the hook until the runner timed it out, which on most runners runs the tool. A missing binary is still allowed unless `POLICYD_REQUIRED` is set.
- `ljos onboard --harness claude` writes `"onFailure": "block"` on the seat's `PreToolUse` hook, and adds it to a hook an earlier onboard wrote. Claude Code runs a tool when a hook fails, times out or cannot start, so a missing `ljos` binary, a crash or a hang let every command through the gate. Claude Code reads the field from v2.1.295. A runner table names the events with `hook_fail_closed`; prompt and session hooks still let the prompt through when they fail.
- The README opens with what the seat does in plain words, lists every runner with a guard hook, and installs with the new script.
- `packset-embed` is optional. The doctor lists the encoder as `info` when it is missing, a sitting no longer says the encoder is down, and the install lines leave it out, so no part of the seat needs ONNX Runtime. Search matches words until an encoder runs.
- `scripts/install.sh` installs the seat in one command. It downloads each program from its latest GitHub release, refuses one whose SHA-256 sum differs from the published one, builds from source where a target has no release, runs `ljos onboard` for each runner it finds, and shows the guard refusing a force push. `--with-embed` adds the encoder; `--from-dir` copies local builds. Each release attaches the script as `install.sh`, so `curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh` fetches the latest one and runs it.
- The integration page has a table of what each runner does when its hook fails, times out or prints something that is not JSON, with the source for each cell, and names the cells no source documents.
- `ljos onboard --harness` takes `copilot`, `factory`, `qwen`, `crush` and `cline`, and `gemini`, `windsurf` and `kiro` now get the guard hook beside their MCP entry. Each hook runs `ljos hook --runner NAME`, which turns the runner's payload into the one the seat reads and answers in the runner's own shape: Copilot CLI's `permissionDecision`, Gemini CLI's `decision`, exit 2 for Windsurf and Kiro, Cline's `cancel`, and Crush's `decision`. Factory and Qwen Code read the Claude shape. Where a runner cannot ask, an ask is a deny.
- `ljos hook --runner NAME` denies when the hook it runs crashes, exits non-zero without an answer, or runs past 12 seconds. Before, the relay answered such a failure as an allow, and the runner ran the command.
- `ljos sitting` hands back expired claims before it claims. A claim quiet longer than `LJOS_LEASE_SECS` (900 by default, 0 turns the step off) goes back to ready under its holder's actor when its conversation is gone, or when no hold record on this host names it; a claim whose runner is still alive is kept, since ljos does not renew a lease while a conversation works. claimdag only applies a lease when something runs `reclaim`, and nothing in the seat did, so a crashed conversation kept its node and its holder stayed busy until a person released it. The sitting prints each node it handed back or kept under `== reclaim`.
- The prompt hook keeps its 2.5 s rerank budget. The pack client gives a reranked search at least sixty seconds, so the budget never applied: on a fresh seat the first prompt after a second memory waited on the writer's cross-encoder download, hit the hook's 8 s deadline and got nothing. The reranked search runs on its own thread now, and past the budget the fused search the hook already made answers.
- An `LJOS_CITE` issue decides a push from seat ballots alone. The check ran `vissue consensus --gate` over every ballot, so persona and Jev ballots, left out of the count, could still block a push their seats agreed on. Now every seat ballot has to name the same option, and one has to come from a seat other than the one pushing; open or closed does not matter. Persona and judge ballots neither pass nor block it. A refusal names the seat ballots it counted (`counted 1 for ship, 1 for hold`) and how many persona and judge ballots it left out.
- `ljos consensus` opens with what a push cite on the issue decides, then prints one settle, the model crate's, as advice. The tracker's settle (`vissue consensus`) runs under `--compare`, or when `ljos-consensus` is not installed. Without `--compare`, `ljos consensus` files no dissent ticket; `vissue consensus ID --file-dissent` files them. `--json` keeps the push cite at the top level of one document, and the MCP tool takes `compare` too.
- `ljos consensus` says on how many ballots a confidence was stated, and that no settle and no push cite weighs it. `ljos learn ID --outcome OPTION` scores it once the outcome is known.
- `ljos doctor` marks the policy row `warn` when `ljos-policyd` was built without phronesis, or is too old to say. It said `ok`, so a seat checking commands with the table alone looked the same as one with the full law. The row names the fix, and a warning does not fail the doctor.
- The install line names `vissue-mcp`. The `vissue` plugin in the Claude Code marketplace runs it as the tracker server, and the line installed `vissue-cli` only, which does not carry it, so that server failed to start. `ljos doctor` lists `vissue-mcp` as optional, with the line that installs it.
- CI checks formatting and runs clippy with warnings as errors. The three warnings develop carried are gone.
- The seat's hook answers once when two registrations run it, such as the Cursor plugin beside an earlier `ljos onboard --harness cursor`, or a runner that also loads Claude's settings. A stop, a session end, a compaction and a failed tool now drop a copy with the same payload inside ten seconds, as the prompt and the tool result already did; a permission check is always answered. `ljos onboard --harness cursor` finds the plugin in `~/.cursor/plugins` and writes no server, hooks or skill, and names a hooks file an earlier onboard left.
- The prompt hook no longer waits on the search model. On a new machine packsetd downloads the model on the first search, and the first prompt used to spend its whole 8 s deadline on it and say nothing. When the pack reports an encoder that has not answered yet, the hook starts one search in the background, skips its own, and says once a session that memory is starting. Session start does the same, so the model is usually loaded by the first prompt.
- `ljos consensus` settles a persona whose anchor is below 0.5 at 0.5, the anchor every other voter takes when a persona votes. Under Friedkin-Johnsen with equal trust the share of its ballot a voter keeps is its weight, so a persona at 0 beside two seats at 0.5 tied their 2-to-1 vote alone (`ljos-consensus settle`: 0.500 to 0.500). A persona can still move more than a seat, and trust rows still weigh voters as they say. When a persona or a judge votes, the settle also prints the seat ballots a push cite counts and, apart, the ballots that are advice, so the count an `LJOS_CITE` check reads is on the same screen as the settle.
- A here-document fed to `python3 -`, `node -`, `perl` or `ruby` goes to `ljos-policyd` as that interpreter's program, so `python3 - <<EOF` with `shutil.rmtree` in it meets the same refusal as `python3 -c`. Seat rules still read the body as data.
- The README's first run shows the deny a seat prints. `ljos-policyd` answers a force push first, with its own rule name, so the example now quotes that answer in the runner's full JSON, and the seat's own rule is shown on a command only it covers.
- The repository is a Cursor plugin as well as a Claude Code plugin. Its manifest is `.cursor-plugin/plugin.json`. It names the tool server in `mcp.json`, the Cursor hooks in `hooks/cursor.json`, the skills, a rule, the commands and an agent. `/add-plugin leidarljos/ljos` in Cursor's agent chat installs it, and so does a clone in `~/.cursor/plugins/local/ljos`. The new skills cover setup, remembering, a sitting, filing, decisions and mail. The new commands are `/remember` and `/doctor`. The `ljos-panelist` agent casts one persona's ballot. Claude Code loads the same skills, commands and agent. When the programs are missing, the launcher says how to install them, and Cursor's permission hooks let the command run with that message instead of blocking every command.
- `ljos hook` answers Cursor's `beforeReadFile` and `beforeMCPExecution` with `allow`. It printed nothing on them, and `ljos onboard --harness cursor` registers both with `failClosed`, which Cursor documents as blocking the call when the hook prints nothing. The seat judges those calls on `preToolUse`, which Cursor fires for every tool.
- Persona ballots no longer count toward an `LJOS_CITE` issue in the push check. The issue has to settle on seat ballots alone: at least one, all for the option the settle names. A persona's ballot and a Jev ballot cast for a persona are left out, and a roster that cannot be read leaves the cite standing on nothing. Before, a seat could vote with personas it wrote on its own issue and cite the result for its own push. Personas still count in every other settle.
- The prompt hook matches a memory on words of three letters or more, with a plural `s` taken off. It counted only words of four letters or more. `Use pnpm, never npm` stayed out of `npm install lodash`, and `migrations` did not meet `migration`, even with the pack ranking each preference first and all three scorers agreeing. Common three-letter words still do not count, such as `the`, `and`, `use` and `add`.
- An `LJOS_CITE` issue needs a ballot from a seat other than the one pushing. An issue closed as a decision counted as a cite with no check of who closed it, and the tracker does not record that, so the pushing seat could close its own decision, or have a persona close it, and cite it. A closed decision now stands when another seat voted on it and every seat ballot names the same option, persona and Jev ballots left out. A settle that stands on the pushing seat's ballot alone no longer counts either. The person backs a push by approving it or by voting.
- `ljos onboard` lists the host key as a signer in an existing deed store, through `deedar host accept`: the key it writes, and an existing key too, so running onboard again fixes a seat onboarded before. A store made before the key existed refused every deed the key signed until the person edited its layout. A deedar without the verb leaves the step failed, and `ljos doctor` names the line to add.
- A deed cited alone no longer lets a push through. The push check took `LJOS_CITE=ACCESSION` for any current deed, and the pushing seat can mint one with `deedar create` just before the push, which went around the rule that a cite needs a ballot from another seat. The cite has to name an issue now.
- `ljos onboard` makes the default deed store (`$XDG_DATA_HOME/deedar/store`) and a tracker at `$XDG_DATA_HOME/vissue/tracker`, and writes the `root` line in vissue's `config.toml` that names it. Before, a fresh seat had neither: `deedar create` refused with `no store at ~/.local/share/deedar/store`, and vissue said the home directory was not a tracker. A store or tracker named by `DEEDAR_URL`, `VISSUE_ROOT`, `ISSUE_ROOT` or an existing `root` line is left alone.
- `ljos receive --since` takes the head this seat kept at the last receive from the same sender, and `--keep FILE` writes the head of the bag it checks, for next time. `ljos handover --since-size N` puts the bridge from the size the receiver kept into the bag as `data/deeds/bridge.txt`. `--since` used to pass a bridge the sender made straight to `deedar check`, and the docs passed `head.txt`; a bridge alone proves nothing, because the sender picks both of its ends. This needs a deedar whose `check` has `--keep`. The MCP tools take `keep` and `since_size`.
- `ljos file` and the hooks no longer take another seat's held issue as their own. A hold record matched on the runner process alone, so a demo started from an agent's shell made that agent file under the demo's issue. A process match now needs the same seat, and a hold record names its claim graph, so a record from another graph never matches.
- `ljos onboard` with no runner prints the server entry alone, so the output parses as JSON. It printed `ok`, `json` and a tab in front of the entry. The README says which runners register `Stop`: `grok` and `antigravity` do, and `claude` and `codex` need it added to `hook_events`. It now says the push check runs only under an ask rule on `git push*`. The getting-started page shows what `learn` and `receive --import` print today.
- `ljos consensus` gives every voter with no anchor of its own, the seat included, an anchor of 0.5 when a persona votes on the issue. Under Friedkin-Johnsen the seat sat at susceptibility 1 and kept none of its ballot, so one anchored persona took the whole settle: a seat for `combmnz` against a persona at 0.3 for `rrf` settled `rrf` 1.000. It now settles `rrf` 0.583, `combmnz` 0.417. A ballot with no persona stays plain DeGroot. The settle prints in words (shares, who leads, polarization, influence, and the share of its ballot each voter keeps), and `--json` prints the model crate's JSON. The MCP tool returns the words beside the JSON.
- The install line is `cargo binstall --locked`. binstall passes `--locked` to `cargo install` when no tarball fits the target, so a source build takes the versions in `Cargo.lock` and not the newest that semver allows. `ljos-hud` also builds from source on a target with no tarball, as `ljos` does. A test checks that the tarball the release uploads for each target is the one the binstall metadata asks for, with every binary inside.
- A shell runner's env file sets `LJOS_SESSION_ID` (kept when the runner already set one), so every shell it opens claims under one holder. Before, each shell hashed its own process into a new holder, and `ljos finish` from a second shell was refused as `not assignee`. That refusal now names the holder, from the hold record or the tracker, and the `--assignee` that finishes under it.
- The env file sets `LJOS_SESSION_ID` only when the runner exports no conversation id of its own. A runner that exports one (`*_SESSION_ID`, `*_THREAD_ID`, `*_CONVERSATION_ID`) now holds per conversation, and its shells hold what its MCP server holds; before, the env file's id was hashed in with the runner's, so the two disagreed. A runner that exports none still shares `NAME-shell` across conversations. `ljos seat` says so on a `shared` line, and a sitting resumed under that name from another live process names the process and how to split the holder.
- A lesson the person types with `ljos remember` takes effect at once, like `ljos prefer` and `ljos rule`, so the hook shows it from the next prompt it bears on. Before, it was an episode the hook left out until `ljos due` and `ljos graded` promoted it the next day. From inside an agent runner, `ljos remember` still writes an episode. A lesson the agent proposes at `ljos finish` or `ljos findings --remember` waits for `ljos accept`, which writes it as standing. Under an agent runner, `accept` takes only a proposal another conversation filed, so the conversation that started a worker can accept the worker's lesson and an agent can't accept its own. `ljos remember` prints which of the two it wrote.
- The doctor requires `packset-embed`. The install line now has it in the README, on the getting-started page and in the docs index, so a seat that follows one of them passes the doctor on day one. A missing binary names its `cargo install` line with or without an answer from crates.io. A seat binary the doctor lists but does not require (`ljos-hud`, `ljos-consensus`, `packset-mcp`) shows `info` when absent instead of `no`, matching the exit code it already gave.
- `ljos consensus` reads a tie from ljos-consensus 0.8, which reports it as `settled: false` with `converged: true`, as a tie, not as an iteration that stopped short. Output from 0.7 reads as before.
- `cargo binstall ljos` builds from source on a target with no release tarball. The `compile` strategy was off, so binstall failed on x86_64 macOS, aarch64 Linux and Windows. cargo-quickinstall stays off, since its build has `ljos` without `ljos-mcp`.
- `ljos-mcp` lists seventeen core tools by default: memory, the consent form, tickets, claims, mail, votes and the doctor. All forty-seven cost about 15,000 tokens of tool definitions and instructions on every session; the core set costs about 6,100 (o200k_base count). `LJOS_MCP_TOOLS=all` lists every tool, and `LJOS_MCP_TOOLS=core,island` adds named ones. The EasyBuild tools `ljos_bump_plan` and `ljos_findings` are out of the default. A tool that fails now answers with a result marked `isError` that carries the reason, where it was a JSON-RPC error that a client shows as a bare internal error.
- A seat rule sees the commands inside `sh -c SCRIPT` (and `bash -lc`, `sh -c -- SCRIPT`, `bash -o pipefail -c`) and inside `env -S STRING`, under any prefix. The prefixes taken off a command are now `sudo`, `doas`, `env`, `time`, `nohup`, `exec`, `command`, `nice`, `timeout`, `setsid`, `stdbuf` and `xargs`, each with its flags and their values, so `sudo -u deploy git push -f` is the push it runs. A `git push*` rule now meets `sh -c "git push -f"` and `command git push -f`; `command -v git` only looks a name up, and text quoted under any other command is still data.
- A seat rule sees the commands inside `$(...)`, backticks, `<(...)`, `>(...)` and a `( ... )` subshell, nested to eight levels, and the substitutions an unquoted here-document expands. `eval ARGS` and `find -exec` count as commands, and so does the command after `if`, `then`, `do`, `{`, `!` and `coproc`. The command name is read without quotes or a path, and git's options before the subcommand are skipped, so `'git' push`, `/usr/bin/git push` and `git -C repo push` meet a `git push*` rule. The same lines go to `ljos-policyd` and the push check. Single quotes, an escaped `$` and a quoted here-document stay data.
- The hooks no longer tell every conversation to file an issue. A conversation that holds no issue hears nothing about the tracker unless `LJOS_TRACK_WORK=on` is set in the environment or in `~/.config/ljos/env`; with it on, the first tool result says to open one with `vissue q` and sit, and a runner that registers `Stop` holds the turn once as before. The reminder to record work on an issue a conversation sat on is unchanged. The message named `ljos file`, which every runner using the seat's hooks showed after `ljos onboard`, the Cursor plugin included.
- A seat rule also sees a command behind a `case` pattern or in a function body, a word in `$'...'` quotes or with quotes inside it (`git 'push'`, `git pu''sh`), a line continued with a backslash, the wrappers `ionice`, `flock`, `chronic`, `unbuffer` and `watch`, the scripts of `trap`, `alias`, `su -c`, `script -c`, `flock -c` and `nix-shell --run`, a here-string, here-document or `echo` piped into a shell, a `git -c alias.NAME=VALUE` alias, and the line `ssh` runs on its host. A line nested past eight levels is refused whole instead of read part of the way. The reference lists what a rule reads and what it does not.
- The seat guard takes every wrapper off before it looks, as the rules do, so `nice tmux send-keys` and `timeout 5 xdotool type` no longer pass it. It refuses `ljos approve` in any command an agent runs, and a write to a file under the approvals directory, where it had guarded only the directory.
- ljos builds against `packset-client` 0.13.0, up from 0.9.20. The client API ljos calls has not changed; the new client brings `packset-core` 0.13.0 with it.
- On a shared host the seat never uses another user's pack writer or runtime files. On Linux the pack client reads who owns the socket on its port: a `PACKSET_PORT` another user listens on is refused, and with nothing set a seat whose default port 8761 is another user's moves to its own port, 18761 plus its user id modulo 10000, where `packset ensure` starts its writer. Before, a fresh home read and wrote whichever packsetd answered 8761. Without `XDG_RUNTIME_DIR`, holds, seat records and approvals go in `/tmp/ljos-UID` (mode 0700) instead of a `/tmp/ljos` every user shared; a name another user or a symlink holds is skipped for `~/.cache/ljos/run`. ljos-hud reads the claim graph through claimdag 0.7.1, whose defaults are per user the same way.

## 0.28.0 (2026-10-10)

- A sitting opens with no encoder. The pack ranks by words alone, the sitting says so, and `finish` still declines a weak island. A refusal names the rows that stopped it. `ljos doctor` still fails on a missing encoder.
- `ljos send` and `ljos inbox` read the writer's version before they probe. packset 0.12.1 refuses an empty message for its text, so the probe passed there, and `inbox` printed `nothing unread` on a pack that cannot keep mail.
- doctor's install hints say `cargo install --locked`. The encoder hint names the Open Neural Network Exchange (ONNX) Runtime that `packset-embed` links.
- A tracker with no remote says `no remote, kept local` after each commit, not `push refused`. `onboard` says it added the hook once it has.
- `ljos accept` writes an agent lesson the writer held. Filing posts the atom, and a `held as proposal` answer is the proposal. Accept calls `/v1/proposals/accept`, so the lesson becomes live, the acceptance is recorded, and a retry does not leave another held proposal. `remember` or `prefer` of the same text still satisfies the proposal and closes it.
- `ljos hook --prompt` prints `mail could not be checked` when the pack does not answer, and still exits 0.
- `ljos deed --remove ACCESSION --ticket ID` drops that citation. The removal is appended to the ticket. The heading is not rewritten.
- A phrase match files a correction as a proposal, and not from pasted or quoted text. A Jev admission still needs the two scorers to agree and the score floor. `due --judge` names a hold and does not grade it. `ljos judge-score` joins the judge log with outcomes. A ballot is recorded as `judge:<model>`, and one model is one voter. `consensus` does not read a judge's forecast. Choice confidence is `(K·p_max − 1)/(K − 1)` on every backend. The cache stores a hash, mode 0600. A chat backend needs `usd_per_mtok_in`.
- `finish --lesson` and `findings --remember` file origin `agent-derived` as proposals. The prompt hook files a correction as a proposal. `ljos accept` writes the atom. `remember` and `prefer` stay `user-declared`. An import from `receive` or `sync` is `peer`. A writer without the direct proposal path keeps the proposal locally.
- `ljos inbox` exits non-zero when the pack does not answer, including a writer that refuses kind `message`. `ljos send` names the packset version that keeps mail before it posts. `ljos hook --prompt` writes a receipt for each message it shows. `ljos inbox` does not.
- Seats message each other through the pack. `ljos send` writes to a seat or a group. `--interrupt` leads the next prompt. `--issue` threads the message on a vissue issue. `ljos inbox` lists unread mail. `ljos read` writes the receipt. `ljos reply` answers the sender and keeps the issue. The prompt hook prints unread mail. A shell seat polls `ljos inbox`. `ljos sync` carries the atoms in the sealed log.
- A shell-only runner (`ljos onboard --harness grokbot`, or `shell`) writes the skill and an env file that sets `LJOS_SEAT`. It registers no MCP server and no hook. `ljos hook --prompt` reads stdin as the prompt. `ljos hook --fail-on-deny` and `ljos policy --fail-on-deny` exit 1 on a deny. Without the flag the exit stays 0.
- Runners now include Windsurf, Zed, VS Code, Claude Desktop, the Gemini CLI, Amazon Q and Kiro. Cursor's IDE and its `agent` CLI share `~/.cursor/mcp.json`.
- `ljos supervise` reads one runner exit and prints `restart`, `stop`, or `give-up`. Permanent restarts either way, transient only on a crash, temporary never. Three restarts inside the window give up. A restart prints `herdr agent start persona-NAME`. The command does not start the child.
- `approve ID` in the chat grants the request when the prompt carries the same conversation under a longer session string. A different conversation still grants nothing.
- A hook's pack listing fails at 2 s so the local permission check can still refuse the command. The permission hook does not exit on a clock. An empty exit would allow it.
- A tracker that keeps one file per issue is read and committed as that file. `ljos` follows vissue 0.20: when `issues/.ledger` is present, a sitting finds the issue there, and the tracker commit names `issues/<id>.org` rather than the project board.
- A persona hand can carry an actor message (`cast`, `call`, `event`, or `exit`). The inbox file is still the markdown `hand` writes. Herdr is up when `herdr workspace list` exits 0.
- `docs/orgmode/integration.org` places the seat among the memory systems it learns from and names the events each runner registers. `docs/orgmode/architecture.org` says how a persona pane, the actor message and `ljos supervise` fit together.
- The bound on decision panel members did not hold under dash: the opener's script counted running members with `jobs` in a command substitution, where dash lists none. The opener now starts the next member as one exits.
- `ljos claim --next` claims a node the way `ljos claim ID` does, so `ljos complete` and `ljos release` find the holder. Claiming a node minted for a tracker id moves that issue to STARTED under the claimant.
- The smoke and herd scripts drop their pack workspace on the way out.

## 0.27.0 (2026-10-09)

- `ljos claim --next` claims the next ready node. `--role` and `--slack` are the affinity and the depth slack. The `claimdag` binary on `PATH` has to be one that serves `claim-next`.
- A decision panel starts at most `LJOS_PANEL_CONCURRENCY` members at once. Unset, that is the machine's parallelism clamped to 4. `LJOS_MAX_PARALLEL` is the same knob. `0` starts every member.
- The status line's due count keeps its 300 ms bound: the lean listing ignored `PACKSET_TIMEOUT_MS` and waited up to thirty seconds on a writer that did not answer. A writer that answers busy is asked again up to three times, after 50, 150 and 450 ms, and the listing stops when its caller's timeout runs out.

## 0.26.1 (2026-10-09)

- `ljos doctor` counts Cursor's hook as installed when `beforeShellExecution`, `preToolUse`, `beforeMCPExecution`, `beforeReadFile` and `postToolUseFailure` are in Cursor's file and Claude's settings carry the seat hook, or when Cursor's file carries every event. The reference page says the same.

## 0.26.0 (2026-10-09)

- An ask a runner cannot show as its own prompt is granted by the client's consent form (`ljos_request_approval`), by replying `approve ID` in the same conversation, or by `ljos approve ID` in a terminal.
- Persona panes open through `[[tool]]` adapters, which `harnesses.toml` declares beside the runners. Herdr and tmux come as shapes, and `ljos doctor` has a `panes` row. Half the herdr calls the seat made had gone stale against herdr 0.9. Every pane fell back to tmux without a warning. The pane resumes a runner that exits non-zero, at most three times a minute. `examples/runtime-panel` has the vote that picked them.
- Among the multiplexers, the pane-typing guard knew only tmux's `send-keys` and herdr's old `send`. It now also refuses an approval sent through herdr's `agent prompt`, `send-keys`, `send-text`, `pane run` or a `terminal session control` fed on stdin. It knows tmux's `paste-buffer` and `pipe-pane -I` too, zellij's `write-chars`, wezterm's and kitty's `send-text`, and screen's `stuff` and `paste`.
- A runner that reports a failed tool (Grok Build's `PostToolUseFailure`) gets up to three standing claims back. Each shares two or more content words with the command and its error.
- A compaction takes every memory handed over so far off the session's record, so a later prompt can bring it back; if two or more were handed over, up to eight of them fire first. The next delivery names the issue the conversation still has claimed. The claude shape now registers `PreCompact` and `SessionStart`, and so do the plugin's hooks; Claude Code hears that issue as the session resumes.
- `ljos agents` and `onboard` write each persona as an agent definition: `~/.grok/agents/ljos-NAME.md`, or a file in `~/.claude/agents` or `~/.cursor/agents`. A runner spawns one by name. It briefs, casts one ballot before reading the others, notes why and stops. Grok Build's own parser, `AgentDefinition::parse` in `xai-grok-agent`, accepts every file.
- `ljos statusline` prints a status-bar line for Grok and Claude Code.
- Where the shape has a `headless` argv, a panel's members run through it on the seat's own runner.
- A brief tells a persona to cast before reading another ballot, and names the issue in its ballot line where it printed `{issue}`.
- Cursor is a built-in runner. `ljos onboard --harness cursor` registers the server, the skill, the agents, and a flat Cursor hooks file. Events Claude's settings already run stay there. `beforeShellExecution`, `preToolUse`, `beforeMCPExecution`, `beforeReadFile` and `postToolUseFailure` are still written, and the permission events fail closed. The hook reads Cursor off its payload and answers in Cursor's fields: `permission`, `user_message`, `agent_message`, `additional_context`, and `followup_message`. A paste into a pane is refused even when that segment does not spell approve. Grok onboard ends with the pack and the host key.
- Claude Code gets the shell check. The built-in claude shape and the plugin's hooks register `PreToolUse` on `Bash`, `Edit`, `Write`, `MultiEdit`, and `NotebookEdit`. The deny rule the README's Five minutes section writes now refuses a force push in Claude Code; onboarding had registered only the prompt, tool-result, session-end, and subagent events.
- A runner started on an entry script (`node pkg/dist/index.js`) takes its seat name from `pkg`, not from `index` or `dist`.
- The smoke and the unit tests keep a runner session the shell inherited (`*_THREAD_ID`, `*_CONVERSATION_ID`) and a named `LJOS_SEAT` out of their scripted seats. The smoke's quiet greps read their whole input under pipefail.
- The prompt hook's median answer fell from 239 ms to 38 ms over eight prompts: it runs the cross-encoder only when the first stage's top twenty hold a claim it could hand over. A prompt that gets claims pays 50 to 90 ms more. The answers did not change: `diff -r` over two `scripts/hook-bench.sh` runs found them the same byte for byte.
- `learn` shrinks each voter's accuracy toward the panel's pooled accuracy by empirical Bayes before it weighs the voter. The first outcome leaves the voters alike; a long record keeps the differences it shows. Plug-in log odds lost to a plain count by 6.4 points on seven similar voters at three outcomes, and shrunk ones by 1.2 (the consensus crate's `correlation_history`).
- `learn` keeps the option each issue closed on, as a pack `outcome`. `consensus` discounts the voters those outcomes show erring together once five issues have one; it names how many outcomes it read and how many independent voices remain. Five clones of one judge beside four voters then settle right 0.780 of the time at five outcomes and 0.813 at a hundred. Plug-in log odds reach 0.719 and 0.708, the significance-tested discount alone 0.736 and 0.805.

## 0.25.5 (2026-10-06)

- A subagent is told the issue its parent has claimed and asked to vote or note on it even when the task omits the id. The first tool of a turn with nothing claimed says to file, sit, and start subagents that record on the filed issue.
- A background tracker push stays visible to doctor after ljos exits. The log name remains the ljos pid. The push shell pid is written beside it, and the row stays healthy while that shell or a git child of it is alive.
- An ignored tracker file is reported with the ignore rule that keeps it out of the commit. A clean tracked file still reports nothing to commit.
- Grok Build receives an ask rule as `ask` on `decision` and `permissionDecision`, and shows its in-chat permission prompt. A runner that cannot ask still has the ask rewritten to a deny, with a one-use request id.
- Claude Code installs the seat from the leidarljos marketplace in this repository. `claude plugin marketplace add leidarljos/ljos`, then `claude plugin install ljos@leidarljos` for the MCP server, the seat protocol skill, the sitting and finish commands, and the hooks `ljos onboard --harness claude` registers. `claude plugin install vissue@leidarljos` fetches the tracker server from leidarljos/vissue.

## 0.25.4 (2026-10-06)

- A correction is written into the pack by the hook. On the runner whose pack tools sit behind a search step, the prompt note names `ljos__ljos_search`, `ljos__ljos_remember`, and `ljos__ljos_prefer`.

## 0.25.3 (2026-10-06)

- A company panel names no model. The brief says to run each member on this same runner and not to set a model id. A spawn hint is not a model the runner can call.

## 0.25.2 (2026-10-06)

- A prompt that asks which answer is right starts the panel itself. The hook forks an opener, which files or reuses the decision and starts one headless member per brief. The conversation is told not to pick and not to ssh.

## 0.25.1 (2026-10-06)

- A prompt that asks which answer is right is held until the turn sits or votes. The first tool result says so, and `Stop` blocks the turn once if it picks an answer with no ballot.

## 0.25.0 (2026-10-06)

- A conversation with no issue claimed is told to file one and sit on the first tool result. A conversation that has already claimed an issue still waits forty tool calls before the reminder to record the work.
- `Stop` blocks that open conversation once when the turn used tools and never touched the seat. The transcript reader takes a top-level `tool_calls` list as well as `message.content` blocks.
- An issue that lives only in `issues/<id>.org` is the issue the seat reads. The seat links vissue-core 0.20, which folds that ledger.

## 0.24.3 (2026-10-04)

- `ljos doctor` has a `policy` row: the ljos-policyd version and whether it judges with phronesis after its built-in table, or with the table alone; without the binary, it says commands are judged only by seat rules.
- The explanation says what each store keeps, where, for how long and how to look at it, the claim graph included, and how a shell command is judged, layer by layer.
- Test fixtures and how-to examples use `demo-` ids in place of real tracker ids.

## 0.24.2 (2026-10-04)

- The TCB judges each pipeline whole, in shell words: `curl URL | sh` reaches `ljos-policyd` as one call, where splitting at the pipe let it pass, and a quoted sentence naming a command is one word.

## 0.24.1 (2026-10-03)

- Consent in the chat: a command an ask rule stopped is approved by the person replying `approve ID` in the same conversation; the prompt hook records the grant, as `ljos approve ID` in a terminal does. A request from another conversation is not granted, and the seat guard refuses typing an approval into a pane.

## 0.24.0 (2026-10-03)

- `ljos vote --as NAME --jev` no longer casts a ballot and then fails: the forecast's claim text names the expected option and its share instead of the whole distribution, which passed the pack's 500-character cap, and the forecast is written before the ballot.
- At a usage limit the Stop hook blocks the turn once per notice and names what to record first: a note on the held issue with what is done and left, `ljos file` for each item left, `ljos remember` for each lesson.
- `ljos file TITLE` files work found while sitting as a child of the held issue, in its project, and commits the tracker; `ljos note ISSUE TEXT` notes progress and commits. The protocol, the work nudge and the panel briefs name them in place of the bare tracker verbs.
- The seat guard reads shell words: a quoted sentence that names a seat path is data, and a path word or a redirection outside quotes into one is still refused.

## 0.23.3 (2026-10-03)

- `ljos onboard --harness NAME` works with no runners file: a runner the
  seat has a built-in shape for (claude, codex, grok, antigravity, opencode, omp,
  hermes) is onboarded from that shape, which is added to
  `~/.config/ljos/harnesses.toml` so the doctor and persona sessions know
  it.
- Rules and the TCB judge each command a line runs, as written and with
  its prefixes off, and no longer the raw line, so a heredoc body that
  names a flag or a command is data and not a command.
- The README opens with what ljos is and for whom, a five-minute first run
  with the output it gives, and a glossary; the command list follows.
- `vissue vote ID` with no `--for` and no `--withdraw` reads the tally
  and is not refused; the seat rule on `vissue vote*` had denied a read.
- The seat command named in a tracker-verb deny drops the shell's
  redirections: `vissue vote X --for A 2>&1` is told `ljos vote X --for
  A`, not `ljos vote X 2>`.

## 0.23.2 (2026-10-02)

- A panel seats who speaks to the title. A persona its island reaches
  sits only when its own view uses a word of the title, and the view
  fallback needs two of the title's words: a cvmfs capability decision
  had seated five physics and course reviewers through the island and
  through views that say "change". A panel with nobody to seat says how
  to write the voters it needs.
- `ljos persona --about` takes two or more domains after one flag as well as
  a comma list or the flag repeated.
- The protocol teaches the judge verbs (`vote --jev`, `panel --jev`,
  `due --judge`), persona sessions and `ljos ask`, and the tracker forms
  `vissue append ISSUE "..."` and `vissue update ISSUE -t TAG`; a seat
  had looked for a jev command and found none.

## 0.23.1 (2026-10-02)

- `ljos upgrade` stages its download under `~/.cache/ljos`, on the home
  filesystem; on a host whose `/tmp` is a full, quota'd tmpfs the
  download failed with a write error.
- The seat guard judges the command line `ssh` runs on its host as a
  command of its own: `ssh h '~/.local/bin/ljos --version'` runs the
  binary and passes, `ssh h 'cp x ~/.local/bin/ljos'` writes it and is
  refused.

## 0.23.0 (2026-10-02)

- `ljos upgrade [VERSION]` installs a published release beside the
  running `ljos`: the archive the release workflow built for the tag,
  checked against its published sha256, each binary an executable that
  names the version, every binary checked before any is replaced, and
  the old ones kept as `NAME.bak-pre-VERSION`. The seat's guard keeps
  agents from writing its binaries; this verb moves them forward with
  bytes no agent made.
- A here-document's body is data, not commands: a seat rule no longer
  meets `cargo build` inside `cat > job.sbatch <<'EOF' ... EOF`, which
  refused every job script written that way on a desktop seat. Commands
  after the closing word still count.

## 0.22.3 (2026-10-02)

- Two tasks handed to a persona in one second keep two inbox files; the
  file was named to the second, so the later overwrote the earlier. A
  live test opens a persona's tmux window, hands it two tasks and reads
  both back.

## 0.22.2 (2026-10-02)

- `ljos-mcp` is found beside the running `ljos` before PATH, so the
  doctor, onboard and the seat binary row work from a shell, such as
  ssh, that lacks the install directory on PATH.

## 0.22.1 (2026-10-02)

- `ljos doctor`'s host row fails while the kernel's out-of-memory kill
  count rises, and for a day after. With none in the last day, the row
  says how many since boot. The counter is cumulative since boot, so one old kill had kept
  the row red until the next reboot.
- The prompt's due line counts what came due this week, with the
  backlog's size beside it, and says nothing when nothing new came due.
  A seat with 1263 due claims had been told so every session; the
  count only grew, and a runner had once tried to drain it unread.
- Claude Code's permission hook covers its file tools: the `PreToolUse`
  matcher is `Bash|Edit|Write|MultiEdit|NotebookEdit`, and a file
  tool's cue is the tool and the path it writes, and leaves out the file's text, so
  the seat's guard refuses a write to a seat path and a doc that names
  one passes.
- Two signals before the prompt hook speaks. A claim Jev says bears on
  the prompt goes in when it also names a content word of the prompt,
  or when Jev is sure alone (0.75); on a vague prompt Jev had put an
  unrelated EESSI planning preference in at 0.61. The treat-as-data
  note needs pasted material in the prompt (a pasted block, a fence,
  terminal or log lines, or many lines) beside Jev's injection answer,
  which ran 0.6 to 0.7 on plain requests; and its text lost a run of
  spaces.

## 0.22.0 (2026-10-02)

- An `ask` on a runner that cannot ask prints a request id; the person
  grants that one command with `ljos approve ID`, for one attempt in
  that directory and conversation, within fifteen minutes. `approve`
  refuses under an agent runner (a runner's conversation in the
  environment, or a runner above it) or without a terminal, so an agent
  cannot grant itself consent, and the approval store is a seat path
  the guard keeps agents out of.
- agy's conversation id names its holder, and its prompt is read out of
  the `<USER_REQUEST>` wrapper.
- The seat guards its own law. Before any rule, a shell command that
  writes the seat's binaries (`ljos`, `ljos-mcp`, `ljos-policyd`), its
  config or a runner's hook registration (`~/.codex/hooks.json`,
  `~/.gemini/config/hooks.json`, `~/.claude/settings.json`,
  `~/.grok/hooks/ljos.json`, the opencode and omp plugins), or a file
  tool aimed at one, is refused as the person's to change; reading
  them is not. An agy session had replaced `~/.local/bin/ljos` with a
  script that allowed every command, and its answer broke every other
  runner's prompt hook. agy's permission hook now sees every tool, and a file
  tool names the path it writes.
- `ljos doctor` fails a `seat binary` row when the `ljos` the hooks run
  is not a binary or not the one running the doctor.

## 0.21.1 (2026-10-02)

- A deny on a bare tracker verb names the exact seat command to run in
  its place, arguments carried over: `vissue claim ljos-6c3z` is told
  to run `ljos sitting ljos-6c3z`, `vissue vote X --for A` is told
  `ljos vote X --for A`. The rule's reason had named a placeholder, and
  an agent guessed.

## 0.21.0 (2026-10-02)

- A seat rule's trailing `*` straight after a word continues only past
  a word boundary: `vissue claim*` meets `vissue claim X` and no longer
  refuses the read-only `vissue claims`.
- The docs and the example runners file name the runners and models a
  persona reasons through; the example includes the `claude` and `codex`
  shapes with their `resume` argv.
- Thinkers are personas. `ljos persona NAME --runner RUNNER` gives a
  persona a runner, a `[[harness]]` whose new `start` and `resume` argv
  say how it opens a session and resumes the latest one in a directory,
  that reasons as it in a session it keeps: a herdr pane or a window of the tmux session
  `ljos-personas`, working in `$XDG_STATE_HOME/ljos/personas/NAME` as
  the seat named after the persona, so its memories, ballots and trust
  rows are the persona's. The runner resumes the latest session of that
  directory, so a later hand-off resumes the conversation, and an
  open pane takes the next task in place. `vote --jev` and `panel
  --jev` hand an unsure persona's ballot to its own session; `ljos ask
  NAME "..."` puts a question to it. The 0.19 and 0.20 thinker roster
  (`[escalate]`, `escalate_band`, `command_mode = "prompt"`, `surface`,
  `PERSONA-THINKER` voters) is gone, and so is the review hand-off from
  `due --judge`.
- Repository facts are pack claims. The push check asks `gh` once per
  repository and remembers the answer as a standing claim on
  `repo:OWNER/REPO`; later pushes read the pack. The runtime cache and
  `push.toml` are gone.

## 0.20.1 (2026-10-02)

- The push check counts a version tag (`v1.2`, `0.3.0`) as a release and
  a bookmark tag as nothing; a notes repository with one
  `campaign-sent` tag had asked for a cite on every push.

## 0.20.0 (2026-10-02)

- Thinkers are seats. When a fast judge leaves a ballot or a due claim
  open, ljos hands it to the runners `[escalate]` names, each started
  in a herdr or tmux pane as a seat of its own: it writes its reasoning
  with `vissue note` and casts `ljos vote --as PERSONA-THINKER`, or
  grades with `ljos graded`, so consensus weighs it by its trust row and
  `learn` and `calibrate` move the row. `vote --jev`, `panel --jev` and
  `due --judge` hand off; each hand-off is noted with its pane. The
  0.19.0 inline escalation, which pooled thinkers' JSON inside the
  judge, is gone.
- The push check asks the forge whose a remote is, so no file is needed:
  `gh api` gives the person's push permission, whether the owner is
  their own account, the collaborators and the releases, kept a day. A
  branch push to the person's own unshared, unreleased repository runs;
  a shared, organisation or released one needs a cite; one they cannot
  push to is theirs to run. On a forge the seat cannot ask, the seat counts
  their own namespace under their GitHub name as theirs. `push.toml`
  remains an override.

## 0.19.0 (2026-10-02)

- A thinker never runs unseen: a prompt-mode judge opens in a herdr
  pane, else a window of the tmux session `ljos-judges` (`surface =
  "auto"`), names itself, tees what it prints back to the seat, and
  leaves a shell in the pane for the person to read, attach or stop it.
  No pane system, no answer. `surface = "none"` is for adapters and
  tests.
- Judges layer: when the route's pool is unsure, a probability inside
  `escalate_band` (0.2 to 0.8) or a choice under `escalate_below`, the
  decision goes on to the thinkers `[escalate]` names, runners such as
  `grok -p`, `omp -p --no-tools` or `hermes -z` in
  prompt mode, each answering with a short `why` that the log keeps.
  Every answer is pooled. A hook never escalates. A thinker runs with
  `LJOS_JUDGE=1`, under which the seat's hook injects nothing and the
  argv law still applies.
- An asked `git push` is checked by where it goes. With
  `~/.config/ljos/push.toml` naming `owners`, a branch push to an
  unreleased repository of theirs runs; a push to one with tags, or one
  `shared` names, runs when it cites the decision behind it,
  `LJOS_CITE=ISSUE git push ...`, where the issue settles (`vissue
  consensus --gate`) or closed as a decision, or `LJOS_CITE=ACCESSION`
  for a current deed, and the pass is noted on the issue; anyone else's
  remote, tags, a mirror or a force stay the person's. With no file no
  push is free. Grok had refused 102 pushes in 13 sessions to
  the agents' own repositories under the blanket `git push*` ask.
- A seat rule is tried on every command a shell line runs, past the
  line's start as well: `cd repo && git push` and `FOO=1 git push` meet the
  `git push*` rule. The line splits on `&&`, `||`, `;`, `|` and `&`
  outside quotes, so a commit message naming a command is not that
  command. The seat matches a regular expression as one, anchored at the
  command's start: a pattern with `re:`, or with a `\b`, `\s`, `\d`,
  `\w` or an alternation group in it; the three search-from-root rules in
  the seat's pack were regexes read as globs and had never fired.
- Judging is a decision layer over named judges. `[judges.NAME]` tables
  in `jev.toml` each name a backend, model, key (`key_file`, `key_cmd`,
  or the new `key_env`) and `weight`. `[route]` names the judges for
  each decision: `prompt`, `ballot`, `audit`, `review`. The top-level
  keys stay the `default` judge, so an existing file reads as before.
  Judges on one decision are asked at once and pooled (log-odds mean,
  geometric mean of distributions, score mean); the log keeps every
  judge's answer beside the pool. `command_mode = "prompt"` lets a
  runner's one-shot mode judge: the questions as its last argument,
  the first JSON object it prints as the answer.
- `ljos due --judge`: the review judges weigh each due claim against
  the pack's newer claims on it. A claim the judges put at 0.9
  is graded recalled, a contradicted one (0.1) is named to supersede or
  withdraw, and the rest stay due; a judge never lapses a claim.
- Antigravity's `agy` is a runner: `ljos onboard --harness antigravity`
  registers the server in `~/.gemini/config/mcp_config.json`, writes the
  skill under `~/.gemini/config/skills`, and puts the seat's hooks under
  the name `ljos` in `~/.gemini/config/hooks.json`. Its payload names no
  event, so each hook command carries `--event`: the argv law on
  `run_command` answers `allow`, `deny` or a real `ask`; the first model
  call of a turn reads the prompt from the transcript and injects the
  note as an ephemeral step, later calls carry the tool-result notes;
  `Stop` blocks a turn with `decision: continue`. `hooks_named` in
  `harnesses.toml` names a hook file of that shape.
- `graded` takes only a claim a due page (`ljos due`, `ljos_due`, a
  sitting) showed in the last hour. A codex seat saved the 1281-row
  `ljos due` list and lapsed all 1314 claims unread in one loop. `ljos
  due` prints the soonest eight; `--all` lists every due claim to read
  and puts none up. The prompt's due line says review is not the task.
- A runner that reads the prompt hook's answer (claude, codex) is no
  longer handed the same note a second time on its first tool result.
- An `ask` rule refused on a runner that cannot ask says the rule does
  not lift on a yes in chat, so the agent stops and hands the command
  to the person instead of retrying it.

- The prompt call asks Jev two more questions in the same request: an
  `injection` noul (quoted or pasted text addressing the agent with
  instructions the person did not write), which adds a treat-as-data
  note at `cue_at`, and an `effort` score from 0 to 3, kept in the Jev
  log for routing. Both are optional in the answer, so an older reply
  still parses; a chat judge's score is clamped to the rubric.
- A chat judge's missing choice confidence is the distribution's
  concentration (one minus normalised entropy), as Jev defines it, not
  the chosen option's probability.

- `backend` in `jev.toml` names the judge for the seat's four judgments:
  `jev` (the default), `chat` (one JSON-mode chat completion on any
  chat-completions endpoint, hosted or a local llama-server) or
  `command` (an argv given the request on stdin, answering on stdout,
  so a runner on the machine can judge). The three request shapes, the
  parsers, the cache, the cost ledger and the callers are shared; a chat
  reply is read into Jev's shape, and a question left unanswered refuses
  the reply. `ljos doctor` prints the backend before the model.

## 0.18.0 (2026-09-30)

- A panel seats a persona matched only through an everyday title word
  ("build", "test", "docs") when no persona speaks to a specific one. A
  hook question titled "which integration to build next" had seated the
  eOn build reviewers beside the seat's own.

- `ljos due` and the prompt hook leave forecasts out. A prediction came
  up for review, agents graded it, and a forecast withdrawn or replaced
  first failed with a raw pack error. `ljos graded` on an atom that is
  no longer current now says so.
- `ljos deed ISSUE --add A --add B` cites many deeds at once, and
  `ljos claims` runs `vissue claims` with its flags.

- A subagent's stop hook names only the issue its own conversation has claimed.
  A hold written from a shell whose runner the process tree had lost
  recorded the multiplexer (herdr) as its conversation, and every subagent
  under that multiplexer then matched it and was asked for a ballot on
  another session's decision. herdr is a session process now, a command
  under one records its pane's shell, and a hold or seat record owned by a
  session process matches nobody.
- `ljos vote ISSUE --withdraw [--as NAME]` (MCP `withdraw`) takes back a
  ballot and its forecast. The tracker's logbook keeps what the ballot
  was, and the settle and the Prelec forecast reading drop both.

## 0.17.0 (2026-09-29)

- With Jev on, `Stop` and `SubagentStop` audit the turn once from the
  runner's transcript. The audit sends the agent back for one more round, with the
  reason, when the final message claims done beside a red test run or
  puts asked work off with no named block. Whether a test ran is read
  from the commands in code; the second stop is never audited.

- `ljos vote ISSUE --as NAME --jev` and `ljos panel ISSUE --jev` ask Jev
  for persona ballots. A sure ballot is cast with the option's
  probability as its confidence and Jev's forecast as its prediction.
  An unsure one (under `escalate_below`, 0.8) is left for a subagent. A
  panel casts only when every seat is sure and all agree, since personas
  from one model are correlated. `ljos_vote` takes `jev` over MCP.
  Every Jev answer is logged to `jev-log.jsonl`. With Jev, the hook
  ranks what it judged by Jev's probability. An identical Jev request
  within `cache_days` (7) is answered from a local cache at no cost.

- The prompt hook can ask Jev, TypeSafe's decision model, which
  candidate claims bear on the prompt and whether the prompt corrects
  the agent or puts a choice. One call answers all three and replaces
  the local rerank and the phrase lists. It stays off unless
  `~/.config/ljos/jev.toml` sets `enabled = true` with a `key_cmd` (such
  as `pass show`) or `key_file`. The hook skips prompts under four
  words, prompts with fewer than two candidates, and every prompt once
  the month's spend reaches `monthly_usd`. With Jev on, the local
  cross-encoder is never loaded. `ljos doctor` prints a `jev` row with
  the month's calls and spend. `bears_at` and `cue_at` (0.5) set the
  probability each judgment needs.

- A prompt or tool-result hook answers inside 8 s. It answers with no
  context before its runner can cut it off; an identical call started in the last
  20 s returns at once, so a hook file two runners load runs once per
  event. The reranked prompt search gets 2.5 s, then the lexical search
  answers. Permission hooks are exempt from both.

- A lesson is stored as an episode (`horizon:transient`). It is kept
  and it is not a refresher. A recalled review promotes it to
  `horizon:standing`, and so does consolidation when the lesson
  replaces an earlier claim. `ljos remember --standing` writes the
  rule immediately. A preference is standing. The pack note asks the
  cross-encoder and injects standing claims only. A lesson with no
  horizon tag is an episode.

- A panel matches personas on topic domains. A `sync:` scope written on
  the roster is not a topic, so it does not seat everyone who carries it.

- `ljos vote --expect` records the private forecast of the others on the
  same command as the ballot. Briefs, the company-panel recipe, and the
  subagent stop line name that flag. `ljos predict` still records a
  forecast on its own. Two or more forecasts and `ljos consensus` names
  the answer more popular than forecast.

- A runner that discards prompt-hook stdout still receives the pack note,
  on the first tool result. `Stop` speaks only when the turn ran no tool,
  because its feedback would start another round. An empty later prompt
  does not erase a note that has not been shown. Memory ids and the
  correction, decision, and due nudges are marked seen only once that
  note is emitted.

- `ljos sync` shares the seat's memory across machines through the tracker
  repository. Each repository names a scope and its age recipients in
  `.ljos/sync.toml`, and each machine writes one sealed log under
  `.ljos/atoms/`. The sitting takes the other machines' logs before the
  island, and finish writes this one's. A `[projects]` table sends one
  project's lessons to another scope, and a finish lesson takes the scope
  of the repository its issue lives in. An import skips texts it already
  has, and a clone another host pushed past merges instead of sticking.

- A finish lesson names the issue it was learned on (`issue:ID`), and every
  claim ljos writes carries `source`: the runner, the conversation, the host
  and, when the runner sets one, the turn.

- The hook answers each runner in the contract it speaks: grok's camel-case
  fields, codex's deny-only verdicts and hermes's `pre_llm_call` context.
  grok's hook file runs ljos by absolute path. `ljos onboard` installs
  plugins for opencode and omp, whose hooks are code.

- A conversation is one holder across nested runners and threads, a
  runner's client names (for example `claude` and `claude-code`) are one
  seat through the runner table's `clients` list, and a library's default client
  name is no seat.

- A subagent is told, on its first tool result, the issue its parent has claimed
  and how its result joins it: a ballot `--as` its role, a lesson or a
  note. grok's `SubagentStop` blocks it once while that issue is open, and
  asks for the ballot on a decision. The parent's issue is found under the
  holder the runner's server recorded.

- A decision issue binds `company-panel` in the sitting and writes one brief
  per seated persona; `finish --close` refuses a decision with fewer than
  two ballots. A prompt that puts a choice in its opening is sent to a
  panel once a session. A panel seats personas by the issue's tags and by
  its island only when the island is strong. When only specialists exist
  and none matches, the five whose views use the issue's words most sit,
  and a panel with nobody to seat says so and stops.

- Every verb that writes the tracker commits and pushes it, to every remote
  that carries the branch. Seats sharing one checkout queue their commits
  under `ljos-commit.lock` and wait out another git process's `index.lock`.
  A rejected push merges and pushes once more.

- An `ljos-mcp` whose binary was replaced forwards each tool call to the
  installed one, so an install reaches a running agent.

- The doctor gains a host row (out-of-memory kills, the kernel, the ljos-mcp servers
  and their memory), a probe row that checks a runner loads the ljos tools,
  and tracker-row failures for remotes that disagree and for a clone that
  lacks the merge driver its `.gitattributes` names.

- The sitting's due list puts the claims in its island first, unless the
  island is weak. The timeline reads every store on the reader's local
  day, and the pack is read without its vectors.

- The doctor's tracker row names how many commits the checkout has that
  origin does not. A count that has sat through the push wait fails the
  row; a leftover `tracker-push-*.log` from a refused push is named on it.
  A live background push, or commits younger than the wait, stay healthy.

- `scripts/smoke.sh` walks two seats handing one scratch ticket in
  sequence: A sits, cites a deed, hands over; B imports; A releases; B
  sits; both vote; consensus; finish closes; the tracker git line reports
  the commit. Distinct from the herd's concurrent contention. A second
  sitting on a closed ticket reopens the tracker heading. The HUD
  message enum boxes its snapshot so clippy `large_enum_variant` stays
  clean.

- The tracker push after a claim or finish waits at most 5 seconds
  (`LJOS_TRACKER_PUSH_WAIT`). A pre-push hook that publishes data first
  held every sitting for minutes; a push still running finishes in the
  background and the `tracker git:` line names its log.

- The doctor's host key row fails when the deed store's `layout` does not
  list the key (`deedar host`), instead of reading healthy while every new
  deed fails `evidence`.

- A seat record carries the conversation ids its writer held, and a shell
  refuses a record written under an id it shares with another
  conversation. Two conversations started from one terminal share the line
  editor's session id. A shell could take the other conversation's
  holder, and then could not finish its own claim.

- `ljos` and `ljos-mcp` expand a leading `~` in `ISSUE_ROOT` and
  `VISSUE_ROOT` at start. The linked tracker crate had read such a root
  relative to the working directory, so `ljos finish` could not find a
  ticket `vissue` itself resolved.

- When the tracker is a git checkout, `ljos sitting` commits and pushes
  the ticket's tracker file (that file only) after its claim, and
  `ljos finish` does the same at the end. A closure that stayed in one working tree was lost to every
  other host. `LJOS_TRACKER_GIT=commit` commits without pushing; `=off`
  skips it. A refused push is reported and does not fail the verb.

- On a fresh host the doctor's claim graph row reads ok, `none yet; the
  first claim creates it at DIR`, instead of a failing row in every
  sitting header. Any other claimdag refusal still fails it.

- The doctor's tracker row names the root vissue resolved, its prefix, and
  where the root came from (`ISSUE_ROOT`, `VISSUE_ROOT`, seat config or the
  working directory). A relative or missing root, or one with no prefix
  directory, fails the row.

- Onboarding retains registration and skill result order while initializing
  the pack and host key before configuring the client.

- Forecast scoring accepts the decimal strings emitted by the tracker as
  well as JSON numbers. Invalid probabilities are reported instead of being
  treated as absent forecasts without a word.

- Doctor checks that the installed tracker accepts evidence citations and
  forecast confidence on ballots. An incompatible vote command fails the
  required tracker row, even when its version is listed in the registry.

- `ljos hud` opens the summonable icedtea pane (`ljos-hud`, a workspace
  member): due, claims, trust canvas, island, and the deed rail. The
  `ljos` crate does not link iced. Pane behavior is the 0.16.3 notes.
- Five playbooks (`sit`, `arena`, `land`, `company-panel`, `overnight`) are
  kind `playbook`, weighed not recalled. `ljos playbooks` lists them;
  `ljos playbook ISSUE NAME` binds one and copies the full recipe body.
  The name is a tracker `playbook:` note until finish or release; those
  verbs write an empty `playbook:` sentinel so the next sitting does not
  reprint it. A different name while one is held is refused. `ljos sitting
  ISSUE --playbook NAME` (MCP `playbook`) prints `== playbook` with that
  body before recall; absent a name, a closed-set token in the title else
  `sit`, so a sitting always binds one of the five before claim. Pack
  latest per name is the copy source; built-in bodies seed only when the
  pack has no live atom of that name. Write, list, bind, and copy refuse
  names outside the five.
- The unscoped inbound floor a persona is owed is the seat's own row
  (`from` is the seat). A third-party unscoped row does not skip it.
- `ljos brief` carries three blocks: the bound playbook's full recipe,
  five named principles (split-fence, prove-on-real-surface,
  open-sibling-first, arena-then-compose, one-step-delegate), and the
  arena rubric. `ljos panel` refuses until a playbook is bound.
  Model names on a recipe are optional spawn hints; every panel still
  ends in `ljos vote --as` then `ljos consensus`.
- Writing a persona also writes one unscoped inbound trust row (the seat
  weighs it at 1, everywhere). `--about` on a later trust row only adds
  weight.

## 0.16.3 (2026-09-22)

- `ljos hud` execs sibling `ljos-hud` (`LJOS_HUD_BIN`, same directory, then
  PATH). Missing is 127; `--hide` with nothing running is 0. The `ljos`
  crate does not link iced. The release builds the HUD for
  `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu` only.
- `ljos-hud` is a long-lived icedtea daemon: summon socket
  (`LJOS_HUD_SOCKET`, `$XDG_RUNTIME_DIR/ljos/hud.sock`), overlay
  `me.rgoswami.ljos-hud`, pop-out `me.rgoswami.ljos-hud.window` (P),
  StatusNotifier tray id and `.desktop` `StartupWMClass` are the overlay
  app_id, `--install-desktop`, guest `xdg_activation_v1`. Default
  detaches; `--foreground` stays attached. First-start `--show`/`--toggle`
  keeps `XDG_ACTIVATION_TOKEN` for the owner (hide still unsets without
  activating). Overlay `Closed` hides. Three read-only panes over library
  APIs: due (with `due_at`, overdue when `due_at` is past, later when
  future, ungraded when empty; no exact-equality `due` chip;
  `review_summary`, display-only grade chips),
  claims (assignee, `cas_gen`, occupancy, lease remaining), a trust-graph
  canvas (personas ∪ trust endpoints, stroke by weight, ring by
  `(1 - anchor)`, missing persona a hollow disk, dashed when scoped),
  island (`packset_search` plus `packset_island(cue, false)`; idle until
  enter; `format_island` weak/dense banners; search-down is a banner), and a deed
  rail (`ljos_cli::timeline_events` to `Vec<Event>`; idle until the
  operator enters an issue; tracker rows from
  `vissue_core::agent::show_json`, not `vissue show --json`). Skip chips sit in
  the layout in key order 1-5 (due, claims, graph, island, deeds). Watch
  `work.bin` plus pack `last_write_ts` (Snap writes both; a missing
  `pack_ts` means boot, and no load yet); the 50 ms tick is chrome. A
  habitat that is down is an icedtea banner and a status page; pack-down
  and honest-empty differ, and a claims banner does not blank an up-empty
  graph. HUD sources never call `graded` / `post_atom` / `sweep` /
  `fire=true` / `due_report` / `complete`.
  `ljos doctor` lists `ljos-hud` and does not require it. crates.io
  publishes `-p ljos-hud` after `-p ljos`. The HUD crate does not copy
  the CLI launcher (`resolve_hud_bin` stays in `ljos-cli`).
- `ljos remember` and `ljos search` start the default writer when none is
  answering. `PACKSET_URL=off` stays off. A URL pointed elsewhere is not
  replaced. The crates.io description and keywords name the first command.
- `ljos finish` no longer closes the tracker ticket on `--status done`; a
  sitting ending is not the work being accepted, and a ticket closed early
  released every blocker on it. `--close` (`close` over MCP) closes it.
  The protocol document already said so; the tool now agrees.
- `--gen` on `ljos finish` and `ljos complete` (and `gen` over MCP) is
  optional: absent, the live generation is read off the claim graph. An
  explicit stale generation is still refused.
- `ljos claim`, and so a sitting, marks the tracker as well as the claim
  graph: `vissue claim ISSUE` runs under the assignee's name, so the issue
  reads STARTED and `vissue claims` names who has claimed it. A tracker that
  refuses the name fails the claim with `ljos release` named as the way
  out; a node the tracker does not know is left alone.

## 0.16.2 (2026-09-22)

- Doctor compares the `ljos-mcp` binary to the `ljos` crate. That crate
  builds the binary; the crates.io name `ljos-mcp` stopped at 0.14.0.
- A sitting sweeps the review clock before it prints the due prefix, the
  same sweep `ljos due` already ran.
- `ljos_due` returns the soonest eight claims, the total due, and the
  clock summary. The full list stays `ljos due`.
- A panel that matches no domain seats only the personas that name no
  domain. It does not seat every specialist in the pack.
- `ljos vote` prints a count and says so. The settle stays `ljos consensus`.
- An island prints whose weights it walked, and whether fire rewrote them.
  Activation spreads along links; it is no rank. A persona brief says to
  walk its own island and to fire only after that island was used.
- A search score is a rank from the scorers, named as such. Learn names
  the rows it rewrote and says the call is not a settle. Finish names
  that the island it fires is the seat's.
- A ballot can state a probability and the deeds it used. The probability
  is the voter's initial opinion and, once an outcome is known, a Brier
  score. The score is not a trust weight. A fire records a trace of the
  links it strengthened.
- A stated probability is also scored by the logarithmic score. The
  voter's running record keeps mean probability against the event rate,
  and from the second forecast Murphy's reliability, resolution, and
  uncertainty. Those scores stay off the trust weight.
## 0.16.1 (2026-09-20)

- `cargo binstall ljos` takes the GitHub tarball (`ljos` and `ljos-mcp`)
  and skips cargo-quickinstall, which only had `ljos`.
- `scripts/smoke.sh` runs `scripts/herd.sh` after the four loops, so the
  herd sits beside the smoke in the release check.

## 0.16.0 (2026-09-20)

- `ljos bump-plan BUNDLE --project P --parent I` puts an eb-stack bundle
  on the tracker: one child issue per module the lock builds, blocked by
  the modules built before it along the SBOM's dependency edges, with ids
  that are a hash of module and generation so a rerun keeps what exists.
  `vissue ready` is then the buildable frontier and a sitting refuses the
  rest. `ljos_bump_plan` over MCP; `--dry-run` prints the rows.

## 0.15.0 (2026-09-20)

- A sitting reads the issue's blockers from the tracker before it claims:
  an issue whose blockers are still open is refused, with the blockers
  and their states named, and nothing is claimed. `ljos sitting ISSUE
  --anyway` (MCP `anyway: true`) sits on it regardless and says so. The
  claim graph and the tracker's graph agree on what is workable.

## 0.14.3 (2026-09-20)

- `ljos findings --issue` run twice on one state file cites the deed the
  first run froze instead of failing on the store's refusal.

## 0.14.2 (2026-09-20)

- `ljos findings --issue` reads the accession out of `id=deed-...`, which
  is what `deedar create` prints; the first run against a real campaign
  wrote its lessons and then failed to cite the state file.

## 0.14.1 (2026-09-20)

- A finding names the module whose build failed (`GCCcore-15.2.0` for
  `eOn-2.17.10-foss-2026.1`), read from EasyBuild's own line, as well as
  the recipe the campaign drives; the lesson and its entities carry both.
  A finding a later attempt got past says so in its second sentence
  instead of quoting the campaign's automatic resolution.
- A pack refusal on prose reaches the MCP client with what passes, so the
  second attempt is not a guess; four runners hit the ceiling on their
  first lesson.
- The `ljos` crate is the one published: it carries the `ljos-mcp` binary,
  so `cargo install ljos` and `cargo binstall ljos` install both. The
  `ljos-mcp` package stays in the workspace unpublished; the crate already
  on crates.io stays where it is.

## 0.14.0 (2026-09-20)

- `ljos findings STATE` reads an eb-stack campaign state file and prints
  its typed findings; `--remember` writes one lesson per finding a person
  or a seat resolved (the recipe, the step, the error line, the fix) under
  the recipe's name, its package and the failure class; `--issue` cites
  the state file as a deed. `ljos_findings` over MCP. The bump is in
  the protocol and the how-to: one island per recipe before it is touched,
  the ladder rung by rung, the findings remembered after.
- A claim leaves a hold record beside the claim graph: the name it is
  held under, the seat, the runner process and when. A sitting that finds
  the node held by another conversation now names it and says whether its
  runner still runs; when the holder is this seat's own conversation and
  its runner is gone, the sitting takes the node over. A runner that
  exited without finishing no longer blocks the next run of the same
  seat.
- `ljos_remember` says what the pack refuses: a third sentence, prose
  above readability grade 14; and what a lesson names.
- `ljos onboard --example` carries the runners this seat has carried
  through one piece of work (opencode, hermes, omp, grok beside the two
  generic shapes), each in the shape it takes the server; the how-to says
  which take the memory hook.

## 0.13.8 (2026-09-20)

- A conversation's holder does not move when a second `*_SESSION_ID`
  appears: the first resolution leaves a record under every id set then,
  and a later process carrying one of them and more finds the holder by
  the shared id. A sitting opened under one id is finished under it when
  a line editor has set another since.
- `ljos doctor` says where a registry answer came from: `crates.io
  (cached)` when read from the day cache. It labels a binary ahead of it
  as well as one behind; a cached answer the binary on `PATH` is already
  ahead of is asked again.
- `ljos_policy` returns what `ljos policy` prints: the TCB verdict, the
  rule that fired and the memories the line activates, under `ruling`.
  `ljos_consensus` runs the Prelec forecast reading and the voters'
  standing beside the settle, as `ljos consensus` does.
- The `ljos-mcp` crate page carries the README.

## 0.13.7 (2026-09-20)

- `ljos onboard` registers into a runner's JSON config by pointer: a
  runner table names `config_json`, `json_pointer` and a `json_entry` template
  (opencode's `mcp` object, for one), and registered means the pointer
  resolves.

## 0.13.6 (2026-09-20)

- Every MCP tool answers an object: the ten that answered a bare array
  now answer `{ rows: [...] }`, since a strict client refused the whole
  server over an array output schema.

- A persona's memory tree: `remember --as` and `prefer --as` write into
  the set `persona-NAME`, its own tree for the duplicate and replacement
  rules; `ljos island --as NAME` and the `ljos_island` tool's `as` walk
  the pack through the persona's own link weights and fire those, not
  the seat's. Facts stay one substrate; readings and paths are the
  persona's. `packset-client` 0.9.20.

## 0.13.5 (2026-09-20)

- A finish says when the island fired already this hour (the pack drops
  a second fire of the same claims for an hour, so many seats or
  personas closing sittings on one issue tighten its links once).
  `scripts/herd.sh` runs eight sittings from four seats at once, contends
  one ticket between two seats, and closes all eight in parallel.

## 0.13.4 (2026-09-19)

- The roster, a brief and the panel prompt read only the persona atoms
  (`packset-client` 0.9.17), not every atom in the workspace.
- `ljos doctor` has a memory row: the live count against the cap and
  the claims forgotten, by reason (packset 0.9.17 counts them).
- Writing a persona that the pack already has supersedes its previous
  atom, so moving an anchor or a view leaves one live persona of that
  name; the roster showed one, the pack kept both.

## 0.13.3 (2026-09-19)

- `ljos personas` and the `ljos_personas` tool print the roster the pack
  keeps: name, anchor, the domains each speaks to, its view. The only
  way to see it was the panel prompt.
- `ljos due` sweeps the pack first and says what the sweep did: reviews
  left due past twice their interval lapse, never-recalled claims missed
  three times are forgotten by neglect, and the list that follows is the
  one after that. `packset-client` 0.9.14.

## 0.13.2 (2026-09-19)

- A panel seats only the personas whose domains the issue speaks to,
  read from its title's words and the island it activates; every persona
  sits when none speaks to it. The `ljos panel` brief and the
  `run_a_panel` prompt agree on the roster, and the prompt counts how many
  of the pack it seated.
- `ljos onboard --harness grok` bumps `LJOS_MCP_GENERATION` in the runner
  config when the crate version moved, so Grok's watcher respawns
  `ljos-mcp` and a session restart is not required.

## 0.13.1 (2026-09-19)

- The holder is any `*_SESSION_ID` the runner set, the full value, ahead
  of the process tag. Two ids that share an eight-character prefix occupy
  different slots. A server sitting and a shell sitting of one session
  are one occupancy name.
- `ljos finish ISSUE` with status done (the default) closes the ticket
  in the tracker, so a board never shows TODO over a completed claim and
  hands the work out again. Failed or cancelled leaves the ticket where
  it is. Completing a node alone still does not close a ticket.
- The server's seat record is also kept under each conversation id the
  runner set, and a shell reads it by any id it shares with the
  server. A shell no longer becomes a second holder when a line editor
  writes a session id of its own into it.
- Every write names the seat that wrote it (`seat:<name>` first among
  the entities; a persona's, a habit's or a trust row's entities join it,
  a trust row's stay the deeds it cites). A hit written by another seat
  says so: `[lesson, 3 days ago, from brio]` in the hook and a brief,
  `(from brio)` in `ljos search`, `from` on the `ljos_search` row. Many
  seats share one pack; a reader now sees whose lesson it is reading.
- `packset-client` 0.9.12, whose hits carry entities and whose writer
  keeps a workspace at a live cap.

## 0.13.0 (2026-09-19)

- The doctor keeps each crates.io answer on disk for a day, so a herd of
  seats opening sittings asks the registry once a day per binary rather
  than once a sitting each. A busy refusal names this conversation's
  holder and the release that frees a conversation that is gone.
- Habits: `ljos habit NAME VALUE [--unit U] [--every 7d] [--source S]`
  takes a reading of a number the seat keeps measuring, as a claim of
  kind `habit` that supersedes the earlier reading and carries it as
  `was`, due for its next reading one cadence on; `ljos habit` lists
  them with the change since the last reading and when the next is due.
  `ljos_habit` is the same over MCP.
- The seat names itself. `ljos-mcp` takes the client's name at initialize
  (`acme-cli`, `brio`, whatever the runner says) as the seat and
  leaves a record under the runtime directory keyed by the runner's
  process; `ljos` in a shell that runner opened walks its own process tree
  to the same record, or to the first ancestor that is not a shell, so a
  runner's tools and its verbs are one seat with nothing set and nothing
  in `env`. Two names: the seat, which memory, ballots and trust accrue to
  across conversations, and the holder (`acme-cli-39u`), which this
  conversation's claims are held under. `ljos seat` prints both; the
  doctor's `seat` row does too. `LJOS_SEAT` still overrides. The runners
  file no longer passes `LJOS_SEAT={name}`; `ljos onboard` alone prints
  the one entry any runner takes. A ballot cast without a persona is cast
  as the seat.
- `cargo install ljos` installs `ljos` and `ljos-mcp`.
- Occupancy is `{holder}:{issue}` and the holder is any `*_SESSION_ID` the
  runner set, else the seat tagged with the conversation's process,
  else `LJOS_SEAT`. No product list. Two conversations hold two tickets;
  the same ticket is still one holder. `doctor` prints the session and
  which variable it came from.

## 0.12.15 (2026-09-15)

- Sitting no longer treats a binary behind crates.io as a habitat
  that does not answer. Doctor still names the gap.

## 0.12.14 (2026-09-15)

- The README no longer says the hook never blocks. A TCB deny on
  PreToolUse still blocks.

## 0.12.13 (2026-09-14)

- `ljos remember` and `prefer` print one line (id, kind, due, text).
  They no longer dump the embedding.

## 0.12.12 (2026-09-14)

- `packset-client` 0.9.2, so the seat tracks the writer that prints
  `packset-mcp --version`.

## 0.12.11 (2026-09-14)

- Doctor times out MCP `--version` so a silent stdio server cannot
  hang the seat. `ljos-mcp` depends on the workspace ljos version.

## 0.12.10 (2026-09-14)

- `packset-client` 0.9.1. `ljos-mcp --version` prints and exits.
  Doctor can version the MCP binary. The door install includes
  `packset-embed`.

## 0.12.9 (2026-09-14)

- `ljos doctor` lists every seat binary (`ljos`, `packset-embed`,
  `packset-mcp`, and the rest) with the version on PATH against crates.io. A
  part that is missing or behind is not ok. Encoder and policyd are
  required with the rest.

## 0.12.8 (2026-09-14)

- `ljos doctor` treats the grok runner hook as the frozen events
  (prompt, PostToolUse, Bash rules, SessionEnd), not a stale
  SessionStart list. `~/.config/ljos/env` is loaded when those
  keys are unset, so a shell `ljos` shares the MCP pack. Argv law
  runs only on PreToolUse and argv, not on PostToolUse.

## 0.12.7 (2026-09-14)

- `ljos onboard --harness grok` writes the frozen `~/.grok/hooks/ljos.json`.
  No table in `harnesses.toml` is required.

## 0.12.6 (2026-09-14)

- Grok hook is `ljos hook` only. The prompt's pack text is held and
  emitted once on `PostToolUse`, the event Grok sends. No
  `sync.sh`. `PreToolUse` stays rules-only.

## 0.12.5 (2026-09-14)

- Grok `PreToolUse` no longer remaps to a pack search. The inject
  script exits on a tool call. `ljos hook` on Bash only decides
  rules. The due nudge no longer walks `consolidate` (that sitting
  is what timed the hook out at 20 s).

- `finish` does not fire a weak island (seeds no two scorers agreed on)
  and says why; `island` marks one as weak and as the pack's
  best-connected cluster rather than what the cue is about; `doctor`
  shows the encoder row, since a down encoder is what makes seeds weak.

## 0.12.4 (2026-09-14)

- Grok onboard writes `GROK_SESSION_ID` into the MCP env so the
  occupancy split in 0.12.3 is live for that runner.

## 0.12.3 (2026-09-14)

- Occupancy uses the runner session when the assignee is a shared
  name (`grok`, `seat`, `you`). Two conversations hold two tickets.

## 0.12.0 (2026-09-13)

- `learn` writes rows from each voter's record: hits and misses so far
  as log-odds weights, the same scale `calibrate` writes, carried on the
  rows. On voters of known accuracy the record reaches batch calibration
  (0.929 against 0.934 over 8000 decisions) where Hedge reaches 0.831 and
  Hedge with recovery 0.877. `--rule hedge` keeps the shrink.
- `conflicts` leaves out passes between trust rows, personas, forecasts
  and rules: weighed, not recalled, so not contradictions to judge.

## 0.11.0 (2026-09-13)

- `conflicts` (and the `ljos_conflicts` tool): candidate contradictions
  by geometry, the lowest passes between single memories in the pack's
  embedding landscape, from the optional `landscape` habitat; nothing
  written.
- The hook reads a correction: a prompt that opens with "do you not
  remember", "you should have", "I told you" or a phrase of that kind gets one line,
  once per cue a session, to write the preference or lesson into the pack
  before the work. A correction the pack never held cannot fire.

## 0.10.0 (2026-09-12)

- `doctor` prints a `seat` row: the name this runner claims and votes
  under, and whether it came from `LJOS_SEAT`, `VISSUE_AGENT` or the
  default.
- `calibrate` writes log-odds weights (Nitzan and Paroush): a voter right
  nine times in ten now outweighs one right six times in ten five to one,
  where the linear rule gave three to two; chance earns the floor.
- `search` prints how many scorers named each hit; the prompt nudge counts
  the pairs `consolidate` would close beside the claims due.
- `consolidate` (and the `ljos_consolidate` tool) runs the pack's
  replacement rule over its claims and reports pairs; `--apply` writes.
- The hook injects only hits at least two of the pack's scorers named
  (`ballots` of `of` on a hit), when more than one ran; a claim one
  scorer alone matched on a command line stays in the pack.
- A second `sitting` on an issue this name has already claimed is a sitting
  resumed: the lease is renewed and the verb goes on, where it refused
  with `claim: status claimed`. Held by another seat, the refusal names
  the actor.

## 0.9.0 (2026-09-12)

- Every recalled memory carries its age: the hook's lines, a brief's
  lines, `search` and the island in `sitting` say `today`, `3 weeks ago`,
  `6 months ago` beside the kind, and the hook's lessons run oldest to
  newest behind the preferences. The reader lays what it recalls on a
  timeline instead of a bag.
- `timeline ISSUE` (and the `ljos_timeline` tool): the tracker's logbook,
  the cited deeds and the activated memories as one dated list, oldest
  first, each line with its age and the gap since the line before.
  `sitting` prints the last twelve as `== timeline`.
- Two runners on one host: `--assignee` defaults to `LJOS_SEAT`, then
  `VISSUE_AGENT`, then `seat`; a ballot cast without a persona is cast as
  `LJOS_SEAT` when set; the runners file may write `{name}` in `register`
  and `snippet`, so a registration passes `LJOS_SEAT={name}` to the server.
- `remember` and `finish` say when the pack closed earlier memories for
  the new one (`revises N earlier memories, now closed`), so a revision is
  seen as one.
- `search --as-of TIME` (and `as_of` on the `ljos_search` tool): the pack
  as it stood then, so "what did the seat know when it decided that" has
  an answer.

## 0.8.0 (2026-09-12)

- `scripts/smoke.sh`: every loop on scratch stores, as a check.
- `sitting` prints the island's strongest eight; `island` prints it all.
- A habitat cut off by the seat's own reader closing the pipe is not a
  refusal: `ljos consensus ID | head` ends with no error.

## 0.7.0 (2026-09-12)

- `learn --share S` adds a fixed share of recovery toward one after the
  Hedge step (Herbster and Warmuth), so a voter refuted long ago can come back;
  zero, the default, is plain Hedge.
- `receive --import` tags every imported atom with its sender
  (`from:<signing key>`, or `from:handover` for an unsigned bag).
- `onboard` starts a pack writer when none answers, before wiring the
  runner to it.
- The hook fires the memories it injected during a session together when
  the session ends (the runner's `SessionEnd` event, on by default), so
  what served one sitting is wired for the next.

## 0.6.0 (2026-09-12)

- `ljos hubs`: the claims the pack's link graph turns on, highest first.
- `handover --to user@host:path` copies the sealed, signed bag to another
  seat over ssh; the receiver runs `ljos receive`.
- `predict ISSUE --expect OPTION` records a forecast of the others; with
  two or more, `consensus` prints the answer more popular than forecast (Prelec,
  Seung and McCoy) and, with trust rows, each voter's EigenTrust standing.
- `rule PATTERN --verdict deny|ask --why TEXT`: argv law in the pack. The
  hook returns the verdict as the runner's permission decision on tool
  calls; `policy` prints it beside the line. Over MCP: `ljos_predict`,
  `ljos_rule`.

## 0.5.0 (2026-09-12)

- `ljos search -n N --rerank`: the writer's cross-encoder over the top hits.
- `ljos panel ISSUE --out DIR`: every persona's brief as a file, so a runner
  without MCP can start one subagent per persona.
- `remember --as NAME` and `prefer --as NAME` (and `as` on the tools): a
  persona keeps lessons of its own, which open its next `brief`.
- `handover` signs the manifest with the seat's default host key, as well as
  with one named by `DEEDAR_HOST_SIGNING_KEY`; it went out unsigned while
  `doctor` reported the key present.

## 0.4.0 (2026-09-12)

- The kind of work sets the dynamics: an issue tagged `broad` settles
  under bounded confidence on the model crate; untagged issues run the
  anchored model. On a prompt the hook says, once per session, how many
  claims are due for review. `sitting` no longer waits on the runners'
  command lines; `doctor` asks them beside the seat's own rows.
- `learn` moves a refuted persona's anchor toward one, and a scoped trust
  row that applies replaces the unscoped row of its pair instead of
  adding to it. Islands print claims only; persona and trust atoms are
  weighed, not recalled.
- `ljos brief NAME ISSUE` and `ljos_brief`: the text a subagent playing a
  persona starts from; `run_a_panel` starts each member from it.
- `--version`. A claim on an issue whose earlier sitting finished reopens
  the session node (`claimdag reopen`) and takes it.

## 0.3.0 (2026-09-12)

Memory where the agent acts:

- `ljos hook` reads a runner's hook JSON (or an argv line) on stdin and
  answers with the memories the action activates, preferences first, in the
  runner's `additionalContext` shape or plain lines. `onboard` installs it
  on a runner's prompt event when its table names a `hooks` file, or on the
  events `hook_events` lists, and drops it from the rest; `doctor` shows
  the row. The default was settled by a panel of the seat's personas. `ljos policy` prints the memory beside the
  argv line.

Personas and scoped trust:

- `ljos persona NAME --anchor A --view TEXT [--about DOMAIN]` writes a voter
  with a view; `ljos vote --as NAME` casts as it; `consensus` passes every
  persona's anchor to both settles as `--susceptibility-of`.
- Trust rows carry `about` domains: an unscoped row applies everywhere, a
  scoped one when the issue's title carries the word. `learn` writes rows
  scoped to the entities of the issue's island.
- Over MCP: `ljos_persona`, `as` on `ljos_vote`, `about` on `ljos_trust`,
  and the `run_a_panel` prompt: one subagent per persona, one ballot each,
  then the settle.

The loop runs every time:

- `ljos sitting ISSUE --assignee NAME` opens a sitting in the protocol's
  order (doctor, cards, due, island, recall, claim) and stops at the first
  store down; `ljos finish ISSUE [--lesson] [--outcome]` closes it
  (remember, fire, complete, learn) and says when no lesson was given.
  Both over MCP as `ljos_sitting` and `ljos_finish`.
- `ljos calibrate -p PROJECT` writes trust rows from the project's voting
  history with no truth labels (Dawid and Skene, through
  `ljos-consensus reliability`), so weights move when nobody names an
  outcome.
- `ljos due` lists claims that never entered the review clock as due, and
  ends with one line on the clock: due, scheduled, next.
- `ljos onboard` writes the seat's host key when there is none, so
  handovers go out signed.

For an agent, or the person running one:

- `ljos protocol` prints the sitting protocol: which store answers which
  question, the order of verbs before, during and after the work, and the
  refusals worth knowing. The server serves it at `ljos://protocol` and
  names it first in its instructions.
- `ljos onboard --harness NAME [--dry-run]` registers `ljos-mcp` with a
  runner described in `~/.config/ljos/harnesses.toml` and installs the
  protocol as its skill; `--harness json` prints the entry for any other,
  `--example` the file's shape. `doctor` reports whether each runner named
  is onboarded.
- `ljos release ID --assignee NAME` and `ljos_release` hand a session node
  back unfinished. A claim refused as busy now names the tracker id the
  name has still claimed and the two verbs that free it.
- Nothing needs a variable set: the pack is found on `127.0.0.1:8761`
  (`PACKSET_URL=off` means no pack), the seat's memory is the one
  workspace `seat` from any directory (`PACKSET_WORKSPACE` names another),
  and the host key at `~/.config/deedar/host.key`.
- Every verb and flag has help; every tool description opens with when to
  call it; the claim and finish arguments say they take tracker ids.

## 0.2.0 (2026-09-12)

The seat over four stores, as one command and one MCP server.

- Memory: `remember`, `prefer`, `forget --why DEED`, `search`, `island
  CUE [--fire]`, `due`, `graded ID [--lapsed]`.
- Agreement: `vote`, `consensus` under the pack's trust rows on both the
  model crate and the tracker's verb, `trust FROM TO W --why DEED`, `learn
  ID --outcome OPTION` (Hedge reweighing, rows written whole).
- Work: `claim` and `complete` take a tracker id or a name and map them to
  claim-graph ids; `deed`, `recall`.
- Handover: `handover --out DIR` packs the satchel, the atoms and the deeds
  both cite, seals and signs; `receive DIR [--since] [--import]` checks and
  imports.
- `doctor` reports every habitat, the host signing key included.
- MCP: twenty-two tools with read/write annotations, two resources, two
  prompts that sequence a sitting and a handover check.
- A documentation site and handbook at https://leidarljos.github.io/ljos/.

## 0.1.0

The first seat: remember, prefer, search, evidence, current, deed, recall,
vote, claim, complete, cards, policy, consensus.
