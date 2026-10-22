# A fresh machine

`ljos cloud up` installs `ljos`, `ljos-mcp`, `ljos-policyd`, and `packsetd` when the release lists it, into `~/.local/bin`, and on a cloud VM the encoder `packset-embed`. It checks the SHA-256 sums, puts that directory first on PATH for this process and in `~/.profile` and `~/.bashrc`, then writes `~/.cursor/mcp.json`. The command is `ljos-mcp` when PATH already runs this install as `cloud up` starts, and the absolute path otherwise, since a client that does not read the edited profile keeps the PATH it had. It clones `LJOS_TRACKER_REMOTE`. Run it again and it refreshes the clone and the MCP command, rewrites env-owned wake entries from `LJOS_WAKE_<SEAT>_URL`, and refreshes the scope `cloud` age identity from the named secret.

`sh scripts/install.sh --cloud` is the same bootstrap. When the binaries are already installed and `ljos cloud up` answers, that command is all that runs. `--force` downloads again. From this repository, if the installed `ljos` has no `cloud up`, the script builds this tree's `ljos` and `ljos-mcp` and leaves them in place.

`ljos cloud up` and `install.sh --cloud` arrive in the next release; the latest release has neither. Until then they work from `develop`: run `sh scripts/install.sh --cloud` in a checkout of it.

## Environment

`LJOS_TRACKER_REMOTE` is the https git URL of the shared tracker. Unset, `cloud up` installs the binaries and does not turn push mode on.

`LJOS_TRACKER_GIT=push` commits each tracker write, fetches, rebases with the per-issue field merge, and pushes. It retries a few times and never forces. A failed push stays committed. The next write and the end of the session try again. `commit` commits and does not fetch or push. `off` skips the commit.

`ljos send` and `ljos reply` in push mode push the sealed log from the detached wake before the wake fires, so the woken seat does not read an empty inbox. The send does not wait on that push. A failed push is a line in the wake log, and the wake still runs.

The wake envelope names the tracker commit that holds the log. A command sees it as `LJOS_WAKE_COMMIT`, and a webhook as `commit` in the JSON. The field is absent when no log commit was made.

The woken seat runs `ljos inbox --sync COMMIT`. That fetches until the remote has the commit, for up to 30 seconds (`LJOS_INBOX_SYNC_WAIT`), or else takes the remote head, imports the sealed logs, and lists. With no remote the flag only lists.

`LJOS_SEAT` is the seat name. `ljos cloud up` appends the run id when one is present, at most 16 characters. On GitHub Actions that id is `GITHUB_RUN_ID`. Otherwise it is `CURSOR_AGENT_ID`, `CLOUD_AGENT_ID`, `AGENT_ID`, `CURSOR_CONVERSATION_ID`, or `CODESPACE_NAME`. `LJOS_SEAT=ridley-ci-a` with run `555001` is the seat `ridley-ci-a-555001`. Unset, the seat is the harness name and that id, or the machine name when there is no id. A seat left in `~/.config/ljos/env` by an earlier boot does not hide a new id.

Each seat signs its ballots and its handovers with `~/.local/share/ljos/seats/SEAT/ballot.key`. `cloud up` points `LJOS_SEAT_KEY` and `DEEDAR_HOST_SIGNING_KEY` at that file, and writes `VISSUE_AGENT` as the same seat. The machine key `~/.config/deedar/host.key` stays where it is. A later process on that machine does not sign a ballot or a handover that already has a signature.

`LJOS_CLOUD_AGE_SECRET` names the environment variable that holds the age identity for scope `cloud`. The value is an identity file as `age-keygen` writes it. `ljos cloud up` writes that identity to `$XDG_CONFIG_HOME/ljos/cloud.age`, or `~/.config/ljos/cloud.age` when `XDG_CONFIG_HOME` is unset, with mode 0600. The file sits beside this machine's own `age.key`. It stays out of the tracker, out of `~/.config/ljos/env`, and out of the harness config. `ljos doctor` reports the file. Sync opens scope `cloud`'s logs with this identity and with the machine key, and every other scope's logs with the machine key alone. With the variable unset, `cloud up` leaves any identity file already written where it is.

`ljos cloud up` also writes `LJOS_WRITE_SCOPE=cloud` into `~/.config/ljos/env` and into the process environment. Memory and lessons written on that machine are tagged `scope:cloud`, so they seal into `<host>+cloud.jsonl.age` when the tracker lists `cloud` under `[scopes]`. `cloud up` does not set `default_scope`. An atom with no `scope:` entity stays on the primary log. A primary scope named `cloud` is refused.

`LJOS_TRACKER_DIR` is the clone. The default is `~/.local/share/ljos/tracker`. `LJOS_BIN_DIR` is the binary directory. The default is `~/.local/bin`. `LJOS_RELEASE_DIR` is an offline directory of binaries and a `SHA256SUMS` file, and of the encoder archives with their `.sha256` files. `LJOS_TRACKER_PUSH_WAIT` is how many seconds `ljos doctor` waits before an unpushed commit counts as stuck. The default is 5.

`LJOS_WAKE_<SEAT>_URL` is the webhook that wakes a seat when mail from this machine reaches it. A wake fires on the sender's machine, so a VM that writes to a box seat needs one. `cloud up` writes each into `~/.config/ljos/wake.toml` with `key_env = "LJOS_WAKE_<SEAT>_KEY"` and `source = "env"`, so the bearer key is read from that variable when the wake fires and is never written to disk. `LJOS_WAKE_INKY_URL` and `LJOS_WAKE_INKY_KEY` wake the seat `inky`, and `LJOS_WAKE_GROK_BOT_URL` wakes `grok-bot`. The URL has to be https, or http when the host is exactly localhost, an IPv4 loopback address, or ::1. Pass both as secrets, so neither is in `environment.json`. Each run writes the `source = "env"` entries again. A wake without that source stays, and the line says so. Two variables that name one seat are refused, and the line names both. Other seats' wakes stay.

`ljos doctor` warns while the tracker has commits the remote lacks. A harness MCP command that will not run on this machine fails the `mcp` row. On a cloud VM, a codespace, or CI the row says to run `ljos cloud up`. Anywhere else it names `ljos onboard --harness` for that file, including Cursor's `mcp.json`. A desktop Cursor terminal sets `CURSOR_AGENT=1` and is not a cloud VM. CI counts by value: `GITHUB_ACTIONS=true`, `GITLAB_CI=true`, `CI=true`, `BUILDKITE=true`, `TF_BUILD=True`, `CIRCLECI=true`, or a `JENKINS_URL`. `false` is not a job. The row reads every harness file ljos writes.

A `STARTED` claim whose process is gone, older than the lease (`LJOS_LEASE_SECS`, default 900 seconds), goes back to `TODO` when `ljos cloud up` or a sitting runs under `LJOS_TRACKER_GIT=push`. The release is pushed with the other tracker writes. A claim this machine still holds is kept. The hold is the nearest program above the command that is not a shell, and it keeps only the issue it names, for as long as that program is still running. `ljos claim` commits and pushes that issue the same way a note does.

## The encoder

`packset-embed` gives a VM dense search and bulk encoding. The release workflow builds the Open Neural Network Exchange (ONNX) Runtime 1.28.3 from source with telemetry off and Debian's protoc, links `packset-embed` 0.16.0 to it with the rpath `$ORIGIN/../lib/onnxruntime`, and attaches `onnxruntime-TARGET.tar.gz` and `packset-embed-TARGET.tar.gz`, each with a `.sha256` beside it. Nothing comes from cdn.pyke.io. x86_64 Linux is the one target built.

`cloud up` fetches both archives from the release of its own version and checks both sums before it unpacks either. A sum that does not match installs nothing and stops `cloud up`. The runtime goes to `~/.local/lib/onnxruntime`, beside the binary directory. A release without the archives leaves search lexical.

A cloud VM is a run with `CURSOR_AGENT_ID` or `CLOUD_AGENT_ID` set, or a machine with `/run/cursor/api.sock`. A CI job is not one, including GitHub Actions. Anywhere else `cloud up` downloads no encoder and installs none. `sh scripts/install.sh --cloud` sets `WITH_EMBED=0` and does not build `packset-embed`. Search stays lexical.

## A job step

A GitHub Actions job, including a self-hosted ephemeral runner, uses the same bootstrap as a cloud agent. The job environment sets the stable seat name and the tracker URL. `GITHUB_RUN_ID` is already set. After `cloud up`, `~/.config/ljos/env` holds the suffixed seat. `vissue` reads the process environment, so that file is sourced before the vote. `ljos note` signs the ballot this seat cast and pushes the issue.

```yaml
- name: Seat on the shared tracker
  env:
    LJOS_SEAT: ridley-ci-a
    LJOS_TRACKER_REMOTE: ${{ secrets.LJOS_TRACKER_REMOTE }}
  run: |
    curl -fsSL -o /tmp/ljos-install.sh https://github.com/leidarljos/ljos/releases/latest/download/install.sh
    sh /tmp/ljos-install.sh --cloud
    set -a
    . "$HOME/.config/ljos/env"
    set +a
    vissue vote ISSUE --for ship --confidence 0.8 --used none
    ljos note ISSUE "the job result"
```

The step fetches the latest release's script, which takes `--cloud` from the next release on. Until then, check out `develop` of this repository and run `sh scripts/install.sh --cloud` from it. On this repository the install line is `sh scripts/install.sh --cloud`. The tracker URL stays in the job environment. The runner's git credentials have to be able to clone it.

## Cloud agent environment

`install` runs once, on the image build. `start` runs on every boot. The harness can sync an MCP config from another machine over the file `install` wrote, and the tracker clone has to be fetched again, so `start` runs `ljos cloud up` as well. Nothing waits on `start`.

This repository:

```json
{
  "name": "ljos",
  "install": "sh scripts/install.sh --cloud",
  "start": "\"$HOME/.local/bin/ljos\" cloud up"
}
```

Another repository, once a release contains `cloud up`:

```json
{
  "name": "ljos",
  "install": "curl -fsSL -o /tmp/ljos-install.sh https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh /tmp/ljos-install.sh --cloud",
  "start": "\"$HOME/.local/bin/ljos\" cloud up"
}
```

Set `LJOS_TRACKER_REMOTE` in the environment. The tracker repository has to be one the git credentials on the machine can clone. The URL stays out of `environment.json`. Set `LJOS_CLOUD_AGE_SECRET` to the name of the secret that holds the age identity for scope `cloud`. The identity stays out of `environment.json` too. Until a release contains `cloud up`, use `scripts/install.sh` from this tree.

`cloud up` rewrites the harness MCP command to `ljos-mcp` when PATH already runs this install, and to the absolute path when another copy or none would run. A claim, a note and a vote then push to the tracker, and another clone sees them after a fetch. A `STARTED` claim whose program has exited returns to `TODO` after `LJOS_LEASE_SECS`. A claim whose program is still running stays. The tracker URL has to be one the machine's git credentials can clone.

## Deeds

A cloud seat's finish exports the deeds that seat cited on the issue into `.ljos/deeds/<seat>/`. The bag carries the deed, the inclusion receipt, the file bytes and any supersedes link. The tracker commit takes the bag, so a fetch brings it to the box. A machine that never ran `ljos cloud up` writes no bag on finish.

## Slots

A run's seat is new, and the tracker pins its key the first time it signs. Any writer with push to the tracker can pin a name that way, and can set the author date on that commit, so the date is not what decides. A seat stands when its key was pinned in this tracker before slots began: the pin is in the parents of the commit that brought the first slot note, by commit order. Until a slot is committed, every committed pin stands. A name does not stand by itself on a git tracker: the first writer to sign as a name the tracker has not pinned pins it, so a name pinned after the first slot is a run seat like any other. A standing seat needs no slot and may sign one. Any other seat's ballot is left out of a push cite, and named, unless it is under a slot. The seat that launches a cloud agent or a job writes one on the issue it hands over, before the run's first ballot:

```sh
ljos slot ISSUE ridley-review --model grok-4 --run "$AGENT_ID" --key "$RUN_KEY"
```

`ridley-review` is the run's `LJOS_SEAT`, and the run id is appended the way `cloud up` appends it. A seat that already ends with the run id is refused, because `ridley-review` on that run boots as the same seat. `RUN_KEY` is the public key on the `seat key:` line `cloud up` prints on the run: the run mints its key on its own machine, and only ballots under that key count for it. A writer that pins the run's seat first under another key does not vote as the run. The slot is signed under the launcher's pinned key, and the launcher has to be a standing seat. The push gate counts a ballot the run casts as its own seat on that issue when the slot verifies and that issue's own entry already held the line in the commit that introduced the run seat's pin, and the push cite line `ljos consensus` prints reads it the same way. A copy of the line on another issue does not count, and an author date is not read. The slot also signs the tracker's root commits, so a copy on an issue with the same id on another tracker does not verify there. The push gate reads the same ballots `ljos consensus` counts, where one session is one ballot. A ballot cast `--as` a persona is a persona ballot, signed with that persona's key, and the push gate leaves it out before any slot is read. The settle `ljos consensus` prints reads no slot, and counts either ballot with or without one. A standing seat needs no slot. A tracker that is not a git repository keeps no order to read, so there the names on a list in ljos stand: `disk-saver`, `inky`, and `ljosbot`. The push gate reads one line per seat from `.ljos-seat-keys`: a second line for a seat refuses the pin file, so appending a line does not re-pin a seat. A pin the tracker once committed stays: a pin file that has changed or dropped one refuses every push cite until the line is back.

## A panel

`ljos panel ISSUE --cloud` writes one prompt file per persona under `--out` (default `panel`). Start one cloud seat from each file. The file sets `LJOS_SEAT` and `VISSUE_AGENT` to the persona's slug and `LJOS_TRACKER_GIT=push`, then says to run `ljos cloud up`, which appends the run id and mints the key. It says to load `~/.config/ljos/env` and cast the ballot `--as "$LJOS_SEAT"`, not `--as` the persona, so the ballot is the run seat's and the push gate reads its slot. The slot line is `slot: SEAT model=FAMILY`. Families rotate grok, opus, codex in persona name order. The launcher signs `slot: SEAT model=FAMILY run=RUN` when the run id exists; this command records the line and does not sign it. The file also states the push rules: only the seat's named branches and develop, no ref under `cursor/`, no `gh pr create`, no `gh pr merge`, no force-push, no deleting a ref. It says the shell guard filters a command as it is written and does not see inside a script, that the pre-push hook stops a git push, that `--no-verify` inside a script skips that hook, that git send-pack does not run it, and that a program that calls the GitHub API with the token needs a server-side ruleset. The brief is `ljos brief NAME ISSUE` on the seat after `cloud up`. The prompt file does not carry the pack. The tracker URL stays in the launcher's environment.

## The cloud guard

`ljos cloud up` installs two checks, and they do not see the same things.

The shell guard is a filter on commands as written. It allows `git push` of `develop` or a branch named in the seat's `ljos.toml`, and it refuses `gh pr create`, `gh pr merge`, a force, and a delete. `gh`, `curl`, `wget`, and `httpie` pass when the command is a read this seat runs. A GET or HEAD whose method and body are on the command line passes, and so do `gh run download`, `gh label list`, `gh cache list`, and a GraphQL query. A field on `gh api` is a write, including one attached to the flag (`-fbase=main`), unless the method is GET or HEAD or the call is a GraphQL query. A curl upload is a write even next to `-G`. A curl config is judged with the command. The default one is the file curl 8 loads: the first of `$CURL_HOME/.curlrc`, `$XDG_CONFIG_HOME/.curlrc`, and `~/.curlrc`, then, with `XDG_CONFIG_HOME` unset, `$CURL_HOME/.config/curlrc` and `~/.config/curlrc`, then `.curlrc` in the passwd home. The Windows `_curlrc`, `USERPROFILE`, and `APPDATA` files are not read, since curl on Linux does not read them. A method or a body in the file is a write even when the command line has no token, and a file that cannot be read is a write. A symlinked config is followed, as curl and wget follow it, and the file it reaches is judged when this user or root owns it. A config either client would load that the guard cannot read as such a file, a directory or a fifo included, leaves the target unknown whatever host the command line shows, and so does a wgetrc line the guard cannot read. A curl config line the guard cannot read, a `variable` or `expand-` option, a `{{` in a URL, and a `-K` file that cannot be read may add a URL, so the request may go to GitHub whatever the command line names. Curl options on the command line and in a curlrc are read through one table, abbreviations and `--no-` included. A proxy, `--resolve`, `--connect-to`, a DoH URL, DNS servers, a unix socket, an alt-svc file, a `Host:` header, and an option the table does not name leave the target unknown. The table also names the ordinary TLS, HTTP version, auth, retry, range, trace, and output options as harmless, so a curlrc that holds only those leaves a GET to GitHub and a write elsewhere allowed. So does `-L`, `--location`, or `--location-trusted`, since a redirect can lead to GitHub, and wget, which follows one unless `max_redirect` is 0. So does a `{`, `}`, `[`, or `]` in a URL host, which curl expands into several hosts unless `-g` is given. `-q` or `--disable` as curl's first argument leaves the default file unread. A wget config is judged the same way. A wgetrc line or `-e` command that sets an input file, a base, a proxy, or a `Host:` header may send the request to GitHub whatever the command line names. So do `-i` or `--input-file`, `-B` or `--base`, and a `Host:` header on the wget command line, split, attached, in a short cluster, or abbreviated. `--no-config` leaves the system file, `WGETRC`, and `~/.wgetrc` unread. A proxy in `http_proxy`, `https_proxy`, or `all_proxy`, either case, leaves the target of a write unknown, whether it comes from the process, an `export` on the line, or a prefix, and `NO_PROXY` does not exempt one. The guard reads a config before the client opens it again. A write to the default curl and wget configs, `SYSTEM_WGETRC`, a config a curl or wget on the same line names, or a `WGETRC` or `SYSTEM_WGETRC` the line sets is refused, but a process already running, or a write to the file a symlinked config reaches, can still change it between the two reads. A host spelling that still names GitHub — any case, a trailing dot, userinfo, percent-encoding, `ghcr.io` — is GitHub. So is an address in place of a name, dotted, one number, hex, or IPv6, since GitHub answers there for the name in a `Host:` header. A command the guard does not recognise stays allowed. `python3` using `urllib`, a library such as dulwich, and `openssl s_client` are in that set: the request is not on the command line. `git push origin develop` is allowed at the shell even when the remote is another repository. The branch name is all that line shows. A `git send-pack` written on the line is judged by its refs and its destination repository. A send-pack started inside a script is not read.

The pre-push hook is the git backstop for a `git push` that does not pass `--no-verify`. Git runs the hook `cloud up` installs for that push, including when a script, `make`, `npm`, or another program starts it, and passes the remote URL after `insteadOf` and the refs. The allowlist is the repository and the branch. This seat may fast-forward `develop` and the branches named in its `ljos.toml` on the repository `cloud up` recorded, and any branch of the tracker. A tag, a delete, a push that is not a fast-forward, and `develop` on any other repository are refused. The shell guard refuses `--no-verify` on a line it can see, a `-c` or `--config core.hooksPath`, the `GIT_CONFIG_*` variables that set that key, and a write to the hook or to a gitconfig. It does not read `./ship.sh`. `--no-verify` inside that script skips the hook. `git send-pack` does not run the hook. dulwich does not run it either. A server-side ruleset is what stops those.

`cloud up` records the seat repository once. A later `cloud up` from another clone does not move that record. It points `core.hooksPath` at the hook when nothing else is booked. When a hooks directory is already booked, it chains a pre-push that runs the previous hook and then this one. A repository whose own `core.hooksPath` names some other directory does not run the global hook. `ljos doctor` fails that row unless the effective hook is the installed script or a chain to it. The hook reads its allowlist from its own path. The pusher's `HOME`, `XDG_CONFIG_HOME`, and `PATH` are not where that file comes from.

A program that calls the GitHub API with the token is out of reach of both checks. The shell guard never sees it, and the pre-push hook only runs for git. Stopping that takes a server-side ruleset.
