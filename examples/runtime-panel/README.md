# Runtime panel

Five personas settled one design question on a scratch seat. The options
were a Go rewrite, OTP-style supervision, adapters for the tools runners
live in (herdr, tmux), or fixing the stale herdr calls in place
(`status-quo`). The panel chose tool adapters, which now live in
`crates/ljos-cli/src/tools.rs` and `persona_session.rs`.

`consensus.txt` is `ljos consensus seat-tvpg` before the outcome was known.
`consensus-after-learn.txt` is the same after `ljos learn seat-tvpg
--outcome tool-adapters`, run under the record rule before `learn` shrank
accuracies. One outcome leaves the voters alike now that `learn` shrinks.

## Seats

| Persona | Anchor | Speaks to | View |
|---|---|---|---|
| `perfengineer` | 0.30 | runtime, performance | Measures before believing |
| `maintainer` | 0.20 | runtime, architecture | Weighs migration cost and the crate ecosystem |
| `reliability` | 0.40 | runtime, operations | Reads for what crashes, restarts, is lost |
| `integrator` | 0.50 | integration, harnesses | Meets each runner and multiplexer where it is |
| `newcomer` | 0.80 | onboarding, runtime | Counts binaries, runtimes and config files |

```
ljos persona maintainer --anchor 0.2 --view "..." --about runtime --about architecture
ljos playbook seat-tvpg company-panel
ljos panel seat-tvpg --out panel-runtime
```

## The ballots

One subagent ran each brief, all in parallel. The brief told it to cast
before reading another ballot and carried none: recall prints neither the
votes nor the logbook. The personas measured from their own views and wrote
their reasoning with `vissue note`.

| Persona | Ballot | Confidence | What it measured |
|---|---|---|---|
| `maintainer` | tool-adapters | 0.62 | 31,349 lines and 290 tests a port would redo; per-runner herdr kind data belongs in `[[harness]]` |
| `integrator` | tool-adapters | 0.62 | 3 of 6 herdr call shapes dead under herdr 0.9; the pane-typing guard missed `agent prompt` |
| `reliability` | tool-adapters | 0.60 | unwatched children: silent fallback, a crashed runner reported alive, dropped member pids |
| `perfengineer` | status-quo | 0.55 | the gate is 3.7 ms; a prompt hook is 190 ms, 98% waiting on packsetd; Go saves 0.3 ms |
| `newcomer` | status-quo | 0.60 | 21 binaries and 3 daemons already; Go or BEAM would add a runtime |

Nobody chose the Go rewrite or OTP supervision.

## The readings

| Reading | tool-adapters | status-quo |
|---|---|---|
| count | 3 | 2 |
| Friedkin-Johnsen settle, persona anchors | 0.679 | 0.321 |
| the tracker's anchored mean | 0.670 | 0.330 |
| surprisingly popular: actual less predicted | +0.102 | +0.092 |
| after `learn` under the record rule, earned self-trust | 0.818 | 0.182 |

The weights after that `learn` are maintainer 0.344, reliability 0.258,
integrator 0.215, perfengineer 0.142 and newcomer 0.040. They are worth 3.95
equal voices.

## What the panel found besides its answer

- The pack closed four of five persona inbound rows, and four of five
  forecasts, as rewrites of one another: `a weighs b at 1.000.` and
  `a weighs c at 1.000.` share five of their seven tokens. The surprisingly
  popular reading needs two forecasts, so it never ran. packset now keeps
  claims with a different `from`, `to`, `about`, `agent`, `issue`, or `name`
  apart. The readings above ran after that fix; the 20 trust rows the
  outcome wrote stay live beside the five persona inbound rows.
- `herdr status server` exits 0 with no server running. Three of the six
  herdr verbs the seat used exited 2 under herdr 0.9, `agent start` among
  them. The shipped herdr shape detects with a socket call and drives
  `workspace create`, `pane run` and `agent prompt`.
- The pane-typing guard missed herdr's `agent prompt`. It now refuses an
  approval typed that way too, or pasted through tmux or screen.
- Persona panes resume a runner that exits non-zero, at most three times a
  minute, as an OTP supervisor restarts a transient child. There is no new
  daemon to install.
