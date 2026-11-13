# `ljos mesh`

`ljos mesh up` joins this machine to the Headscale mesh at <https://hs.grund.rgoswami.me>.
`ljos mesh down` leaves it.
`ljos mesh acl` prints the access control list policy.
It reads `mesh.toml`.
`up` and `down` do not.

## `ljos mesh up`

```
ljos mesh up [--key-file PATH] [--login-server URL] [--hostname NAME]
```

The key file is `--key-file`, or `LJOS_MESH_KEY_FILE` when the flag is omitted.
The login server is `--login-server`, or `LJOS_MESH_LOGIN_SERVER` when the flag is omitted.
When both are unset the server is `https://hs.grund.rgoswami.me`.
The hostname is `--hostname`.
When that flag is omitted, ljos uses the system hostname.

The join runs `tailscale up` with these flags and no others:

```
--accept-routes=false --accept-dns=false --login-server=URL --auth-key=file:PATH --hostname=NAME --reset
```

`--accept-routes=false` leaves this machine's routes as they are.
`--accept-dns=false` leaves this machine's DNS as it is.
`--reset` lets a later up change those flags without aborting.
The key is passed as `file:PATH`.
ljos does not print the key, write it into a log, or copy the file.
Another process of the same user can still read the file, so use an ephemeral pre-auth key.

Do not pass `--advertise-tags`.
The key already carries the tag.
Headscale refuses a client that asks for the tag again, with `requested tags [...] are invalid or not permitted` (seen on the box, 3 November 2026).

A successful `mesh up`, including one that finds the node already up, prints one line.
The line names this node, its mesh IPv4, its mesh IPv6, and each peer's name and addresses.
It does not log in again when the node is already up.

`tailscale` and `tailscaled` are taken from `PATH` when both are there.
When either is missing, ljos downloads the pinned static build, tailscale 1.90.6 for amd64, from `https://pkgs.tailscale.com/stable/tailscale_1.90.6_amd64.tgz` into `$XDG_STATE_HOME/ljos/mesh` (`~/.local/state/ljos/mesh` when `XDG_STATE_HOME` is unset).
It checks the tarball against the sha256 published at `https://pkgs.tailscale.com/stable/tailscale_1.90.6_amd64.tgz.sha256`.
A mismatch installs nothing.
The downloaded binaries stay after `mesh down`, so the next join does not download them again.

When `/dev/net/tun` is missing, cannot be opened, or this process lacks `CAP_NET_ADMIN`, tailscaled runs with `--tun=userspace-networking --socks5-server=localhost:1055`.
Opening the device node is not enough to create an interface.
`LJOS_MESH_TUN` names a different device node.
1055 is the port in Tailscale's userspace networking guide.
The proxy listens on localhost only.
Otherwise tailscaled uses a kernel TUN device.
A machine with no `/proc` to read the capability from, such as macOS, takes userspace mode.
It keeps its node key in memory (`--state=mem:`) and uploads no logs.

Each call stops after 30 seconds.
That is the `timeout -k 5 30` a sitting already runs under.
`LJOS_MESH_WAIT` sets the same bound in seconds.
A stopped tailscaled has 5 seconds to exit, the `-k 5` of that bound, and is then killed.

A failure prints one line on stderr, names the step, and exits non-zero.
The caller is not stopped.
The socket, the pid and the log live in the state directory above, outside any checkout.
The pid file records tailscaled's start time next to its pid.
When that start time cannot be read, `mesh up` keeps tailscaled running, writes the pid alone and says so.
Before either verb signals tailscaled, it checks that the pid still started at that time.
A pid that no longer names that tailscaled gets no signal.
The verb then drops the pid file and says so in its line.
A pid file an older ljos wrote holds no start time either.
`mesh up` says so, even when tailscaled already answers, because that tailscaled gets no signal and its writes get no label.
`mesh up` does not touch git config, hooks, a checkout or `~/.gitconfig`.

## `ljos mesh down`

`ljos mesh down` runs `tailscale logout` and stops the tailscaled `mesh up` started.
It signals the process in its pid file, and only when that pid still has the start time the file recorded.
Any other pid is left alone, and the line says so.
A second `mesh down`, or a `mesh down` before any `mesh up`, prints `mesh down: not on the mesh` and exits 0.
A failure here is also one line on stderr, and the exit is non-zero.

## The label

While that tailscaled is running, a write ljos records carries `source.mesh` with the hostname and the seat the writer claims.
The label is for consensus to read.
Nothing waits on it, and nothing is stored or refused because of it.
When tailscaled exits, the next write has no mesh label.
The seat is whatever the writer says, so the label records a claim and nothing more.

## `ljos mesh acl`

`ljos mesh acl` reads `$XDG_CONFIG_HOME/ljos/mesh.toml`, or `~/.config/ljos/mesh.toml`.
[examples/mesh.toml](../examples/mesh.toml) is a commented copy.
Each swarm reaches its own nodes on every port.
It also reaches `tag:box` on the four box ports the file lists.
No other rule exists, so no swarm reaches another, and the box reaches no swarm.
The join key carries the swarm's tag.
No user owns a tag, so a node cannot tag itself into another swarm.
`ljos mesh acl SWARM...` adds the same pair of rules for each swarm it names.

## Lane memory

`ljos mesh pull --seat S --from DIR` reads the lane `.jsonl` files in `DIR` into seat `S` and writes a seed at `<tracker>/.ljos/memory/mesh/<seat>/seed.jsonl`.
The lane is the file name without `.jsonl`.
A row already in the pack stays unchanged and is not imported again. The match is the text SHA-256, or that lane plus the row id (`atom_id`, `row_id` or `id`).
The printed line is `imported N, unchanged U, skipped M duplicates`.
`N` is new texts. `U` is texts or row ids already held. `M` is a repeated text in this pull.
New rows land in the seat's import slot labelled `imported,unintegrated`.
`pull` then integrates them. That is the same step as a full `ljos memory integrate`.
Integrate deduplicates each row against memories already in the pack, posts a new one once, and relabels the row `imported,integrated`.
The stored atom carries the packset tags `lane:<name>` and `seat:<S>`.
A lesson with no horizon is standing, so this seat's search ranks it with the seat's own memories.
The pull script runs one command. It does not call integrate itself:

```
LJOS_SEAT=S ljos mesh pull --seat S --from DIR
```

`LJOS_SEAT` is the seat whose later searches read the slot. It is the same name as the seat passed to pull.
`ljos mesh push --seat S --to DIR` writes that seat's memories to `DIR/<lane>.jsonl`.
The lane name is `--lane`, or `LJOS_SYNC_HOST`, or the hostname.
A pull and a push each take a file lock on the directory and on the seat.
A writer that answers 503 is asked again once, after `Retry-After`, with the same deadline.
A second 503, a 408, or a writer that is not answering stops the pull and exits 1.
The line is:

```
Error: mesh: the pull stopped: CAUSE. Lessons already stored stay in the seed. Run `ljos mesh pull --seat SEAT --from DIR` for the rows still missing
```
A 408 uses `the deadline passed before this request ran` as `CAUSE`.
A second 503 uses the writer's line, or `packsetd busy, retrying later`.
A writer that is not answering uses the reason this seat prints for a down writer, and that reason names `cargo install --locked packset` or `packset ensure`.
Lessons already stored stay in the seed, and the next pull continues with the rows still missing.
A row the writer did not take is one of those rows.
[Move memory across lanes](source/howto-mesh.rst) has the commands.
