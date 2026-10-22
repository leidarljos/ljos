# A fresh machine

`ljos cloud up` installs `ljos`, `ljos-mcp`, `ljos-policyd`, and `packsetd` when the release lists it, into `~/.local/bin`, and on a cloud VM the encoder `packset-embed`. It checks the SHA-256 sums, puts that directory first on PATH for this process and in `~/.profile` and `~/.bashrc`, then writes `~/.cursor/mcp.json`. The command is `ljos-mcp` when that directory is already first, and the absolute path when an earlier directory would still win at the moment of the write. It clones `LJOS_TRACKER_REMOTE`. Run it again and it refreshes the clone and the MCP command, rewrites env-owned wake entries from `LJOS_WAKE_<SEAT>_URL`, and refreshes the scope `cloud` age identity from the named secret.

`sh scripts/install.sh --cloud` is the same bootstrap. When the binaries are already installed and `ljos cloud up` answers, that command is all that runs. `--force` downloads again. From this repository, if the installed `ljos` has no `cloud up`, the script builds this tree's `ljos` and `ljos-mcp` and leaves them in place.

## Environment

`LJOS_TRACKER_REMOTE` is the https git URL of the shared tracker. Unset, `cloud up` installs the binaries and does not turn push mode on.

`LJOS_TRACKER_GIT=push` commits each tracker write, fetches, rebases with the per-issue field merge, and pushes. It retries a few times and never forces. A failed push stays committed. The next write and the end of the session try again. `commit` commits and does not fetch or push. `off` skips the commit.

`ljos send` and `ljos reply` in push mode push the sealed log from the detached wake before the wake fires, so the woken seat does not read an empty inbox. The send does not wait on that push. A failed push is a line in the wake log, and the wake still runs.

The wake envelope names the tracker commit that holds the log. A command sees it as `LJOS_WAKE_COMMIT`, and a webhook as `commit` in the JSON. The field is absent when no log commit was made.

The woken seat runs `ljos inbox --sync COMMIT`. That fetches until the commit is present, or the remote head when it is not, imports the sealed logs, and lists. With no remote the flag only lists.

`LJOS_SEAT` is the seat name. `ljos cloud up` appends the run id when one is present, at most 16 characters. On GitHub Actions that id is `GITHUB_RUN_ID`. Otherwise it is `CURSOR_AGENT_ID`, `CLOUD_AGENT_ID`, `AGENT_ID`, `CURSOR_CONVERSATION_ID`, or `CODESPACE_NAME`. `LJOS_SEAT=ridley-ci-a` with run `555001` is the seat `ridley-ci-a-555001`. Unset, the seat is the harness name and that id, or the machine name when there is no id. A seat left in `~/.config/ljos/env` by an earlier boot does not hide a new id.

Each seat signs its ballots and its handovers with `~/.local/share/ljos/seats/SEAT/ballot.key`. `cloud up` points `LJOS_SEAT_KEY` and `DEEDAR_HOST_SIGNING_KEY` at that file, and writes `VISSUE_AGENT` as the same seat. The machine key `~/.config/deedar/host.key` stays where it is. A later process on that machine does not sign a ballot or a handover that already has a signature.

`LJOS_CLOUD_AGE_SECRET` names the environment variable that holds the age identity for scope `cloud`. The value is an identity file as `age-keygen` writes it. `ljos cloud up` writes that identity to `$XDG_CONFIG_HOME/ljos/cloud.age`, or `~/.config/ljos/cloud.age` when `XDG_CONFIG_HOME` is unset, with mode 0600. The file sits beside this machine's own `age.key`. It stays out of the tracker, out of `~/.config/ljos/env`, and out of the harness config. `ljos doctor` reports the file. Sync opens logs with this identity and with the machine key. With the variable unset, `cloud up` leaves any identity file already written where it is.

`LJOS_TRACKER_DIR` is the clone. The default is `~/.local/share/ljos/tracker`. `LJOS_BIN_DIR` is the binary directory. The default is `~/.local/bin`. `LJOS_RELEASE_DIR` is an offline directory of binaries and a `SHA256SUMS` file, and of the encoder archives with their `.sha256` files. `LJOS_TRACKER_PUSH_WAIT` is how many seconds `ljos doctor` waits before an unpushed commit counts as stuck. The default is 5.

`LJOS_WAKE_<SEAT>_URL` is the webhook that wakes a seat when mail from this machine reaches it. A wake fires on the sender's machine, so a VM that writes to a box seat needs one. `cloud up` writes each into `~/.config/ljos/wake.toml` with `key_env = "LJOS_WAKE_<SEAT>_KEY"` and `source = "env"`, so the bearer key is read from that variable when the wake fires and is never written to disk. `LJOS_WAKE_INKY_URL` and `LJOS_WAKE_INKY_KEY` wake the seat `inky`, and `LJOS_WAKE_GROK_BOT_URL` wakes `grok-bot`. The URL has to be https, or http when the host is exactly localhost, an IPv4 loopback address, or ::1. Pass both as secrets, so neither is in `environment.json`. Each run writes the `source = "env"` entries again. A wake without that source stays, and the line says so. Two variables that name one seat are refused, and the line names both. Other seats' wakes stay.

`ljos doctor` warns while the tracker has commits the remote lacks. A harness MCP command that will not run on this machine fails the `mcp` row. On a cloud VM, a codespace, or CI the row says to run `ljos cloud up`. Anywhere else it names `ljos onboard --harness` for that file, including Cursor's `mcp.json`. The row reads every harness file ljos writes.

A `STARTED` claim whose process is gone, older than the lease (`LJOS_LEASE_SECS`, default 900 seconds), goes back to `TODO` when `ljos cloud up` or a sitting runs under `LJOS_TRACKER_GIT=push`. The release is pushed with the other tracker writes. A claim this machine still holds is kept. The hold is the nearest program above the command that is not a shell, and it keeps only the issue it names, for as long as that program is still running. `ljos claim` commits and pushes that issue the same way a note does.

## The encoder

`packset-embed` gives a VM dense search and bulk encoding. The release workflow builds the Open Neural Network Exchange (ONNX) Runtime 1.28.3 from source with telemetry off and Debian's protoc, links `packset-embed` 0.15.3 to it with the rpath `$ORIGIN/../lib/onnxruntime`, and attaches `onnxruntime-TARGET.tar.gz` and `packset-embed-TARGET.tar.gz`, each with a `.sha256` beside it. Nothing comes from cdn.pyke.io. x86_64 Linux is the one target built.

`cloud up` fetches both archives from the release of its own version and checks both sums before it unpacks either. A sum that does not match installs nothing and stops `cloud up`. The runtime goes to `~/.local/lib/onnxruntime`, beside the binary directory. A release without the archives leaves search lexical.

A cloud VM is a run with `CURSOR_AGENT_ID` or `CLOUD_AGENT_ID` set, or a machine with `/run/cursor/api.sock`. A GitHub Actions job is not one. Anywhere else `cloud up` downloads no encoder and installs none. `sh scripts/install.sh --cloud` sets `WITH_EMBED=0` and does not build `packset-embed`. Search stays lexical.

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

On this repository the install line is `sh scripts/install.sh --cloud`. The tracker URL stays in the job environment. The runner's git credentials have to be able to clone it.

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

`cloud up` rewrites the harness MCP command to `ljos-mcp` when this directory is already first on PATH, and to the absolute path when an earlier directory would still win. A claim, a note and a vote then push to the tracker, and another clone sees them after a fetch. A `STARTED` claim whose program has exited returns to `TODO` after `LJOS_LEASE_SECS`. A claim whose program is still running stays. The tracker URL has to be one the machine's git credentials can clone.

## Deeds

A cloud seat's finish exports the deeds that seat cited on the issue into `.ljos/deeds/<seat>/`. The bag carries the deed, the inclusion receipt, the file bytes and any supersedes link. The tracker commit takes the bag, so a fetch brings it to the box. A machine that never ran `ljos cloud up` writes no bag on finish.

## Slots

A run's seat is new, and the tracker pins its key the first time it signs. Any writer with push to the tracker can pin a name that way, so a seat first pinned after slots began counts toward a push cite only under a slot. The seat that launches a cloud agent or a job writes one on the issue it hands over, before the run's first ballot:

```sh
ljos slot ISSUE ridley-review --model grok-4 --run "$AGENT_ID"
```

`ridley-review` is the run's `LJOS_SEAT`, and the run id is appended the way `cloud up` appends it. The slot is signed under the launcher's pinned key, and the launcher has to be a seat pinned before slots began. `ljos consensus` and the push gate count the run's ballot on that issue when the slot verifies and the run's key was pinned after it. A seat pinned before slots began needs no slot. On a tracker that is not a git repository, no seat needs one.

## A panel

`ljos panel ISSUE --cloud` writes one prompt file per persona under `--out` (default `panel`). Start one cloud seat from each file. The file sets `LJOS_SEAT` and `VISSUE_AGENT` to the persona's slug and `LJOS_TRACKER_GIT=push`, then says to run `ljos cloud up`, which appends the run id and mints the key. The slot line is `slot: SEAT model=FAMILY`. Families rotate grok, opus, codex in persona name order. The launcher signs `slot: SEAT model=FAMILY run=RUN` when the run id exists; this command records the line and does not sign it. The file also states the push rules: no ref under `cursor/`, no `gh pr create`, no `gh pr merge`, no force-push. The brief is `ljos brief NAME ISSUE` on the seat after `cloud up`. The prompt file does not carry the pack. The tracker URL stays in the launcher's environment.
