# Runtime panel

Five personas settled one design question on a scratch seat: rewrite the
seat in Go for goroutines, give it an Erlang/OTP-style supervision model,
build a declarative integration system for the tools runners live in
(herdr, tmux), or fix the stale herdr calls in place. The panel chose tool
adapters, and they are what `src/tools.rs` and `src/persona_session.rs`
now hold.

`issue.org` is the decision as the tracker kept it: the body, each
persona's reasoning note and the votes drawer. `consensus.txt` is
`ljos consensus seat-tvpg` before the outcome was known,
`consensus-after-learn.txt` after `ljos learn seat-tvpg --outcome
tool-adapters`.

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

One subagent per brief, in parallel, each told to cast before reading
another ballot (the brief carries none: recall prints neither the votes nor
the logbook). Each measured from its own view and wrote its reasoning with
`vissue note`.

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
| after `learn`, earned self-trust | 0.818 | 0.182 |

After `learn` the settle weighs the ballots by social power: maintainer
0.344, reliability 0.258, integrator 0.215, perfengineer 0.142, newcomer
0.040, worth 3.95 equal voices.

## What the panel found besides its answer

- The pack closed four of five persona inbound rows and four of five
  forecasts as rewrites of one another (`a weighs b at 1.000.` and `a weighs
  c at 1.000.` share five of seven words), so the surprisingly popular
  reading never ran. packset now keeps claims with different `from`, `to`,
  `about`, `agent`, `issue` or `name` apart; with it all 25 trust rows the
  outcome wrote stay live.
- `herdr status server` exits 0 with no server running, and every herdr
  verb the seat used exited 2 under herdr 0.9. The shipped herdr shape
  detects with a socket call and drives `workspace create`, `pane run` and
  `agent prompt`.
- The pane-typing guard now refuses every multiplexer verb that can type an
  approval into another runner's prompt.
- Persona panes now resume a runner that fails, at most three times a
  minute, as an OTP supervisor restarts a transient child, without a new
  daemon to install.
