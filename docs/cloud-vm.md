# A fresh machine

`ljos cloud up` installs `ljos`, `ljos-mcp`, `ljos-policyd`, and `packsetd` when the release lists it, into `~/.local/bin`. It checks the SHA-256 sums, puts that directory on PATH for this process and in `~/.profile` and `~/.bashrc`, then writes `~/.cursor/mcp.json` so the command is `ljos-mcp` on PATH, and clones `LJOS_TRACKER_REMOTE`. Run it again and it only refreshes the clone and the MCP command.

`sh scripts/install.sh --cloud` is the same bootstrap. When the binaries are already installed and `ljos cloud up` answers, that command is all that runs. `--force` downloads again. From this repository, if the installed `ljos` has no `cloud up`, the script builds this tree's `ljos` and `ljos-mcp` and leaves them in place.

## Environment

`LJOS_TRACKER_REMOTE` is the https git URL of the shared tracker. Unset, `cloud up` installs the binaries and does not turn push mode on.

`LJOS_TRACKER_GIT=push` commits each tracker write, fetches, rebases with the per-issue field merge, and pushes. It retries a few times and never forces. A failed push stays committed. The next write and the end of the session try again. `commit` commits and does not fetch or push. `off` skips the commit.

`LJOS_SEAT` is the seat name. `ljos cloud up` appends the run id when one is present, at most 16 characters. On GitHub Actions that id is `GITHUB_RUN_ID`. Otherwise it is `CURSOR_AGENT_ID`, `CLOUD_AGENT_ID`, `AGENT_ID`, `CURSOR_CONVERSATION_ID`, or `CODESPACE_NAME`. `LJOS_SEAT=ridley-ci-a` with run `555001` is the seat `ridley-ci-a-555001`. Unset, the seat is the harness name and that id, or the machine name when there is no id. A seat left in `~/.config/ljos/env` by an earlier boot does not hide a new id.

Each seat signs its ballots and its handovers with `~/.local/share/ljos/seats/SEAT/ballot.key`. `cloud up` points `LJOS_SEAT_KEY` and `DEEDAR_HOST_SIGNING_KEY` at that file, and writes `VISSUE_AGENT` as the same seat. The machine key `~/.config/deedar/host.key` stays where it is. A later process on that machine does not sign a ballot or a handover that already has a signature.

`LJOS_TRACKER_DIR` is the clone. The default is `~/.local/share/ljos/tracker`. `LJOS_BIN_DIR` is the binary directory. The default is `~/.local/bin`. `LJOS_RELEASE_DIR` is an offline directory of binaries and a `SHA256SUMS` file. `LJOS_TRACKER_PUSH_WAIT` is how many seconds `ljos doctor` waits before an unpushed commit counts as stuck. The default is 5.

`ljos doctor` warns while the tracker has commits the remote lacks. A harness MCP command that will not run on this machine fails the `mcp` row, and the row says to run `ljos cloud up`. The row reads every harness file ljos writes, not only Cursor's `mcp.json`.

A `STARTED` claim whose process is gone, older than the lease (`LJOS_LEASE_SECS`, default 900 seconds), goes back to `TODO` when `ljos cloud up` or a sitting runs under `LJOS_TRACKER_GIT=push`. The release is pushed with the other tracker writes. A claim this machine still holds is kept. The hold is the nearest program above the command that is not a shell, and it keeps only the issue it names, for as long as that program is still running. `ljos claim` commits and pushes that issue the same way a note does.

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

Set `LJOS_TRACKER_REMOTE` in the environment. The tracker repository has to be one the git credentials on the machine can clone. The URL stays out of `environment.json`. Until a release contains `cloud up`, use `scripts/install.sh` from this tree.

`cloud up` rewrites the harness MCP command to `ljos-mcp` on PATH. A claim, a note and a vote then push to the tracker, and another clone sees them after a fetch. A `STARTED` claim whose program has exited returns to `TODO` after `LJOS_LEASE_SECS`. A claim whose program is still running stays. The tracker URL has to be one the machine's git credentials can clone.

## Slots

A run's seat is new, and the tracker pins its key the first time it signs. Any writer with push to the tracker can pin a name that way, so a seat first pinned after slots began counts toward a push cite only under a slot. The seat that launches a cloud agent or a job writes one on the issue it hands over, before the run's first ballot:

```sh
ljos slot ISSUE ridley-review --model grok-4 --run "$AGENT_ID"
```

`ridley-review` is the run's `LJOS_SEAT`, and the run id is appended the way `cloud up` appends it. The slot is signed under the launcher's pinned key, and the launcher has to be a seat pinned before slots began. `ljos consensus` and the push gate count the run's ballot on that issue when the slot verifies and the run's key was pinned after it. A seat pinned before slots began needs no slot. On a tracker that is not a git repository, no seat needs one.
