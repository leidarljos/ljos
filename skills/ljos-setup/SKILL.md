---
name: ljos-setup
description: Install the ljos programs and check they answer. Use when a hook or the ljos tool server says an ljos program is not installed, when `ljos doctor` reports a `no`, or when the person asks to set up ljos.
---

# Set up ljos

The plugin brings the hooks, the tool server entry, the skills and the rules.
Those parts call the ljos programs, which you install once per machine.
Check the programs first.

1. Check what is there: `ljos doctor`. If `ljos` is not found, go to step 2.
   If every required row says `ok`, stop: the seat is ready.
2. Install the programs. Prefer prebuilt binaries:

   ```
   cargo binstall --locked ljos packset packset-embed vissue-cli deedar-cli claimdag-cli ljos-policyd ljos-consensus
   ```

   Without `cargo binstall`, use `cargo install --locked` with the same list.
   This builds from source and takes longer. Ask the person before running
   either, since it installs programs on their machine.
3. Run `ljos doctor` again. A `no` on the pack means the memory store is not
   running: `packset ensure` starts it. Run the doctor until the required rows
   say `ok`.
4. Tell the person to start a new chat, or reload the window, so the hooks and
   the tool server pick up the programs.

Do not run `ljos onboard --harness cursor` when this plugin is installed. The
plugin already registers the hooks and the tool server, and onboarding would
register them a second time in `~/.cursor/hooks.json`.

The binaries go to `~/.cargo/bin`. The plugin also looks in `~/.local/bin`.
