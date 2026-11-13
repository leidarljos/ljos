#!/bin/sh
# Take ljos off this machine. The ljos program is not needed.
#
#   curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/uninstall.sh && sh uninstall.sh
#
# With a ljos that knows `uninstall` in the bin dir or on PATH, this runs
# `ljos uninstall` with the same options and stops. Without one (deleted
# by hand, or an older release), it does the work here:
#   - every runner's hooks file loses the entries that call `ljos hook`,
#     and the rest of the file stays (python3 does the JSON; with no
#     python3 a file that holds other hooks is moved aside to NAME.ljos-off)
#   - the ljos MCP entries and skill directories go
#   - the global git hooks are put back to what they were before ljos
#   - the ljos programs are removed from the bin dir
#   - a stub named ljos that answers every hook with an allow is left where
#     the program was and wherever a hooks file named it, so a session that
#     loaded the hook list while ljos was installed is not blocked
#
# Options:
#   --bin-dir DIR     where the programs are (default: $LJOS_BIN_DIR, else
#                     the first of ~/.local/bin ~/.cargo/bin that holds a ljos
#                     program, else ~/.local/bin); the directory of every
#                     ljos path a hooks file names is cleaned as well
#   --keep-programs   leave the programs; runner files and git hooks still go
#   --all             remove the other seat programs too: packset, packsetd,
#                     packset-mcp, packset-embed, vissue, vissue-mcp, deedar,
#                     claimdag
#   --purge           remove the state, data, cache and configuration
#                     directories (tracker, pack and deeds included)
#   --no-stub         leave nothing where ljos was
#   --dry-run         print what would change and change nothing
#   --no-exec         do the work here even when a ljos program is present
#   --help            this text
set -eu

BIN_DIR=${LJOS_BIN_DIR:-}
KEEP=0 ALL=0 PURGE=0 STUB=1 DRY=0 EXEC=1
ARGS=""

say() { printf '%s\n' "$*"; }
die() { printf 'uninstall.sh: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case $1 in
    --bin-dir) [ $# -ge 2 ] || die "--bin-dir needs a directory"; BIN_DIR=$2; shift 2 ;;
    --bin-dir=*) BIN_DIR=${1#--bin-dir=}; shift ;;
    --keep-programs) KEEP=1; ARGS="$ARGS --keep-programs"; shift ;;
    --all) ALL=1; ARGS="$ARGS --all"; shift ;;
    --purge) PURGE=1; ARGS="$ARGS --purge"; shift ;;
    --no-stub) STUB=0; ARGS="$ARGS --no-stub"; shift ;;
    --dry-run) DRY=1; ARGS="$ARGS --dry-run"; shift ;;
    --no-exec) EXEC=0; shift ;;
    --help|-h) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    *) die "unknown option $1 (try --help)" ;;
  esac
done

[ -n "${HOME:-}" ] || die "HOME is not set"

if [ -z "$BIN_DIR" ]; then
  for d in "$HOME/.local/bin" "$HOME/.cargo/bin"; do
    for p in ljos ljos-mcp ljos-policyd ljos-consensus; do
      if [ -e "$d/$p" ]; then BIN_DIR=$d; break 2; fi
    done
  done
  [ -n "$BIN_DIR" ] || BIN_DIR=$HOME/.local/bin
fi

# A real ljos (not the stub this script leaves) that knows `uninstall`
# does the fine-grained work: it has the onboard manifest.
real_ljos() {
  for candidate in "$BIN_DIR/ljos" "$(command -v ljos 2>/dev/null || true)"; do
    [ -n "$candidate" ] && [ -x "$candidate" ] || continue
    if head -c 2 "$candidate" 2>/dev/null | grep -q '^#!'; then continue; fi
    if "$candidate" uninstall --help >/dev/null 2>&1; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}
if [ "$EXEC" = 1 ]; then
  if prog=$(real_ljos); then
    say "Running $prog uninstall$ARGS"
    # shellcheck disable=SC2086
    exec "$prog" uninstall --bin-dir "$BIN_DIR" $ARGS
  fi
fi

CONFIG=${XDG_CONFIG_HOME:-$HOME/.config}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
STATE=${XDG_STATE_HOME:-$HOME/.local/state}
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}

act() {
  if [ "$DRY" = 1 ]; then say "  would $1"; else say "  $1"; fi
}

# Every hooks file the shipped runner table names, one per line. Kept in
# step with HARNESSES_EXAMPLE in crates/ljos-cli/src/lib.rs by a test.
HOOK_FILES="$HOME/.claude/settings.json
$HOME/.codex/hooks.json
$HOME/.gemini/config/hooks.json
$HOME/.cursor/hooks.json
$HOME/.codeium/windsurf/hooks.json
$HOME/.gemini/settings.json
$HOME/.kiro/hooks/ljos.json
$HOME/.copilot/hooks/ljos.json
$HOME/.factory/hooks.json
$HOME/.qwen/settings.json
$HOME/.config/crush/crush.json
$HOME/.augment/settings.json
$HOME/.config/amp/settings.json
$HOME/.agents/plugins/ljos/hooks/hooks.json
$HOME/.openhands/hooks.json
$HOME/.continue/settings.json
$HOME/.grok/hooks/ljos.json
${CLAUDE_CONFIG_DIR:-$HOME/.claude}/settings.json"

# Files that hold an MCP entry for ljos, or a whole-file ljos config.
MCP_FILES="$HOME/.claude.json
$HOME/.config/opencode/opencode.json
$HOME/.omp/agent/config.json
$HOME/.config/Code/User/mcp.json
$HOME/.config/zed/settings.json
$HOME/.config/Claude/claude_desktop_config.json
$HOME/.aws/amazonq/mcp.json
$HOME/.kiro/settings/mcp.json
$HOME/.copilot/mcp-config.json
$HOME/.factory/mcp.json
$HOME/.cline/mcp_settings.json
$HOME/.augment/mcp.json
$HOME/.openhands/mcp.json
$HOME/.continue/mcpServers/ljos.json
$HOME/.roo/mcp.json
$HOME/.kilocode/mcp.json
$HOME/.codeium/windsurf/mcp_config.json
$HOME/.cursor/mcp.json
$HOME/.gemini/settings.json
$HOME/.gemini/config/settings.json
$HOME/.qwen/settings.json
$HOME/.antigravity/mcp.json"

# Skill directories ljos writes; each goes only when its SKILL.md names ljos.
SKILL_DIRS="$HOME/.runner/skills
$HOME/.other/skills
$HOME/.config/runner/skills
$HOME/.config/opencode/skills
$HOME/.hermes/skills
$HOME/.omp/agent/skills
$HOME/.claude/skills
$HOME/.codex/skills
$HOME/.gemini/config/skills
$HOME/.cursor/skills
$HOME/.grok/skills
$HOME/.example-host/skills
$HOME/.agents/skills
$HOME/.codeium/windsurf/skills
$HOME/.config/zed/skills
$HOME/.config/Code/User/skills
$HOME/.config/Claude/skills
$HOME/.gemini/skills
$HOME/.aws/amazonq/skills
$HOME/.kiro/skills
$HOME/.copilot/skills
$HOME/.factory/skills
$HOME/.qwen/skills
$HOME/.config/crush/skills
$HOME/.cline/skills
$HOME/.augment/skills
$HOME/.config/amp/skills
$HOME/.agents/plugins/ljos/skills
$HOME/.openhands/skills
$HOME/.continue/rules
$HOME/.roo/skills
$HOME/.kilocode/skills"

STUB_PATHS_FILE=$(mktemp "${TMPDIR:-/tmp}/ljos-uninstall.XXXXXX")
trap 'rm -f "${STUB_PATHS_FILE:?}"' EXIT INT TERM
note_stub_paths() {
  # The absolute program path a hooks file calls, before " hook".
  grep -oh '[^"'"'"' ]*/ljos hook' "$1" 2>/dev/null | sed 's/ hook$//' >> "$STUB_PATHS_FILE" || true
  return 0
}

# Strip the ljos hook entries and the ljos MCP entry from one JSON file.
# Prints "clean" when the file had none, "empty" when nothing but a version
# is left, else "kept".
HAVE_PY=0
command -v python3 >/dev/null 2>&1 && HAVE_PY=1
strip_json() {
  python3 - "$1" "$DRY" <<'PY'
import json, sys
path, dry = sys.argv[1], sys.argv[2] == "1"
try:
    with open(path) as f:
        doc = json.load(f)
except Exception:
    print("clean"); sys.exit(0)
def hooky(v):
    if isinstance(v, str): return "ljos hook" in v or "ljos-guard-" in v
    if isinstance(v, list): return any(hooky(x) for x in v)
    if isinstance(v, dict): return any(hooky(x) for x in v.values())
    return False
def mcp(v):
    return "ljos-mcp" in json.dumps(v)
def strip(v):
    if isinstance(v, list):
        out = []
        for x in v:
            if isinstance(x, dict) and not any(isinstance(y, list) for y in x.values()) and hooky(x):
                continue
            had = hooky(x)
            x = strip(x)
            if had and isinstance(x, dict):
                h = x.get("hooks")
                if isinstance(h, (list, dict)) and not h:
                    continue
            out.append(x)
        return out
    if isinstance(v, dict):
        return {k: strip(x) for k, x in v.items() if not (k == "ljos" and (hooky(x) or mcp(x)))}
    return v
def prune(v):
    if isinstance(v, list):
        v = [prune(x) for x in v]
        return [x for x in v if x not in ([], {})]
    if isinstance(v, dict):
        v = {k: prune(x) for k, x in v.items()}
        return {k: x for k, x in v.items() if x not in ([], {})}
    return v
new = prune(strip(doc))
if new == doc:
    print("clean"); sys.exit(0)
if not isinstance(new, dict) or all(k == "version" for k in new):
    if not dry:
        import os; os.remove(path)
    print("empty"); sys.exit(0)
if not dry:
    with open(path, "w") as f:
        json.dump(new, f, indent=2); f.write("\n")
print("kept")
PY
}

if [ "$DRY" = 1 ]; then
  say "Taking ljos off this machine (dry run)"
else
  say "Taking ljos off this machine"
fi

say "Runner hooks"
printf '%s\n' "$HOOK_FILES" | sort -u | while IFS= read -r f; do
  [ -f "$f" ] || continue
  grep -q 'ljos hook\|ljos-guard-' "$f" 2>/dev/null || continue
  note_stub_paths "$f"
  if [ "$HAVE_PY" = 1 ]; then
    case $(strip_json "$f") in
      empty) act "remove $f (it held only ljos)" ;;
      kept) act "drop the ljos hooks from $f" ;;
      clean) ;;
    esac
  else
    act "move $f to $f.ljos-off (no python3 to edit it; the other hooks in it are off too)"
    [ "$DRY" = 1 ] || mv "$f" "$f.ljos-off"
  fi
done
# Cline keeps one script per event; ljos marks its own.
for f in "$HOME/Documents/Cline/Hooks/PreToolUse" "$HOME/Documents/Cline/Hooks/UserPromptSubmit"; do
  [ -f "$f" ] && grep -q 'Written by ljos onboard' "$f" 2>/dev/null || continue
  act "remove $f"
  [ "$DRY" = 1 ] || rm -f "${f:?}"
done
# Auggie and Amp run a shell relay beside their settings.
for f in "$HOME/.augment/ljos-guard-auggie.sh" "$HOME/.config/amp/ljos-guard-amp.sh"; do
  [ -f "$f" ] || continue
  act "remove $f"
  [ "$DRY" = 1 ] || rm -f "${f:?}"
done

say "MCP entries"
printf '%s\n' "$MCP_FILES" | sort -u | while IFS= read -r f; do
  [ -f "$f" ] || continue
  grep -q 'ljos' "$f" 2>/dev/null || continue
  if [ "$HAVE_PY" = 1 ]; then
    case $(strip_json "$f") in
      empty) act "remove $f (it held only ljos)" ;;
      kept) act "drop the ljos entry from $f" ;;
      clean) ;;
    esac
  else
    say "  $f still points at ljos, and python3 is not installed, so the file was not edited. Install python3 and run \`sh uninstall.sh\` again, or edit $f and delete the ljos entry."
  fi
done
# the command harness keeps its servers in TOML: the [mcp_servers.ljos] table goes.
if [ -f "$HOME/.codex/config.toml" ] && grep -q '^\[mcp_servers\.ljos\]' "$HOME/.codex/config.toml"; then
  act "drop [mcp_servers.ljos] from $HOME/.codex/config.toml"
  if [ "$DRY" = 0 ]; then
    awk '/^\[mcp_servers\.ljos\]/ { skip = 1; next } /^\[/ { skip = 0 } !skip' \
      "$HOME/.codex/config.toml" > "$HOME/.codex/config.toml.new" \
      && mv "$HOME/.codex/config.toml.new" "$HOME/.codex/config.toml"
  fi
fi
say "Skills"
printf '%s\n' "$SKILL_DIRS" | sort -u | while IFS= read -r d; do
  s="$d/ljos"
  if [ -d "$s" ] && [ -f "$s/SKILL.md" ] && grep -q 'ljos' "$s/SKILL.md"; then
    act "remove $s"
    [ "$DRY" = 1 ] || rm -rf "${s:?}"
  fi
  s="$d/ljos.md"
  if [ -f "$s" ] && grep -q 'ljos' "$s"; then
    act "remove $s"
    [ "$DRY" = 1 ] || rm -f "${s:?}"
  fi
done
f="$HOME/.aider/ljos-conventions.md"
if [ -f "$f" ]; then
  act "remove $f"
  [ "$DRY" = 1 ] || rm -f "${f:?}"
fi

say "Git hooks"
GIT_HOOKS=${LJOS_GIT_HOOKS:-$DATA/ljos/git-hooks}
if command -v git >/dev/null 2>&1; then
  current=$(git config --global --get core.hooksPath 2>/dev/null || true)
  if [ -n "$current" ] && [ -d "$current" ] && [ "$(cd "$current" && pwd -P)" = "$(cd "$GIT_HOOKS" 2>/dev/null && pwd -P)" ]; then
    prev=$(sed -n '1s/[[:space:]]*$//p' "$GIT_HOOKS/previous-hooks-path" 2>/dev/null || true)
    if [ -n "$prev" ]; then
      act "set core.hooksPath back to $prev"
      [ "$DRY" = 1 ] || git config --global core.hooksPath "$prev"
    else
      act "unset core.hooksPath"
      [ "$DRY" = 1 ] || git config --global --unset core.hooksPath || true
    fi
    act "remove $GIT_HOOKS"
    [ "$DRY" = 1 ] || rm -rf "${GIT_HOOKS:?}"
  elif [ -n "$current" ] && [ -d "$current" ] && grep -ls 'ljos git-hook' "$current"/* >/dev/null 2>&1; then
    say "  $current has hooks that call ljos; each keeps its other lines, so edit them by hand:"
    grep -ls 'ljos git-hook' "$current"/* | sed 's/^/    /'
  elif [ -d "$GIT_HOOKS" ]; then
    act "remove $GIT_HOOKS"
    [ "$DRY" = 1 ] || rm -rf "${GIT_HOOKS:?}"
  fi
else
  say "  no git here; nothing to put back"
fi

# The hook server exits once its socket is gone.
RUN=${XDG_RUNTIME_DIR:-}
if [ -n "$RUN" ] && [ -S "$RUN/ljos/hookd.sock" ]; then
  act "stop the hook server ($RUN/ljos/hookd.sock)"
  [ "$DRY" = 1 ] || rm -f "${RUN:?}/ljos/hookd.sock"
fi

write_stub() {
  tmp="$1.stub.$$"
  cat > "$tmp" <<'STUB'
#!/bin/sh
# ljos was uninstalled. This stub stands where the program was, so a
# runner that loaded its hook list while ljos was installed still gets
# an answer: every hook call is allowed. Remove it with
#   rm "$0"
# or reinstall ljos over it.
case "$1" in
  hook)
    cat >/dev/null 2>&1
    case " $* " in *" --git-commit "*) exit 0 ;; esac
    printf '%s\n' '{"permission":"allow","continue":true}'
    exit 0
    ;;
  git-hook|hookd) exit 0 ;;
esac
printf '%s\n' "ljos is uninstalled; $0 is the stub that answers hooks. Reinstall: curl -fsSLO https://github.com/leidarljos/ljos/releases/latest/download/install.sh && sh install.sh" >&2
exit 1
STUB
  chmod 755 "$tmp"
  mv -f "$tmp" "$1"
}

if [ "$KEEP" = 0 ]; then
  say "Programs"
  PROGS="ljos ljos-mcp ljos-policyd ljos-consensus ljos-hud"
  [ "$ALL" = 1 ] && PROGS="$PROGS packset packsetd packset-mcp packset-embed vissue vissue-mcp deedar claimdag"
  # The bin dir, and the directory of every ljos a hooks file named.
  { printf '%s\n' "$BIN_DIR"; sed 's|/[^/]*$||' "$STUB_PATHS_FILE"; } | sort -u | while IFS= read -r d; do
    [ -n "$d" ] && [ -d "$d" ] || continue
    for p in $PROGS; do
      [ -e "$d/$p" ] || continue
      if [ "$p" = ljos ] && [ "$STUB" = 1 ]; then
        act "replace $d/ljos with the stub that allows every hook"
        [ "$DRY" = 1 ] || write_stub "$d/ljos"
      else
        act "remove $d/$p"
        [ "$DRY" = 1 ] || rm -f "${d:?}/${p:?}"
      fi
    done
  done
fi

if [ "$STUB" = 1 ]; then
  # Wherever a hooks file called ljos by an absolute path, a stub answers,
  # so a session that cached that path is not blocked. A real program is
  # never overwritten.
  { cat "$STUB_PATHS_FILE"; printf '%s\n' "$BIN_DIR/ljos"; } | sort -u | while IFS= read -r p; do
    [ -n "$p" ] || continue
    d=$(dirname "$p")
    [ -d "$d" ] || continue
    if [ -e "$p" ]; then
      head -c 2 "$p" 2>/dev/null | grep -q '^#!' || continue
      grep -q 'ljos was uninstalled' "$p" 2>/dev/null && continue
    fi
    act "leave the stub at $p"
    [ "$DRY" = 1 ] || write_stub "$p"
  done
fi

if [ "$PURGE" = 1 ]; then
  say "State"
  for d in "$CONFIG/ljos" "$DATA/ljos" "$STATE/ljos" "$STATE/ljos-policyd" "$CACHE/ljos"; do
    [ -e "$d" ] || continue
    act "remove $d"
    [ "$DRY" = 1 ] || rm -rf "${d:?}"
  done
else
  say "State kept under $CONFIG/ljos, $DATA/ljos and $STATE/ljos (--purge removes it)"
fi
still=0
if [ "$DRY" = 0 ]; then
  left_list=$(mktemp "${TMPDIR:-/tmp}/ljos-uninstall-left.XXXXXX")
  {
    printf '%s\n' "$MCP_FILES"
    printf '%s\n' "$HOOK_FILES"
    printf '%s\n' "$HOME/.hermes/config.yaml" "$HOME/.config/goose/config.yaml" \
      "$HOME/.grok/config.toml" "$HOME/.aider.conf.yml" "$HOME/.codex/config.toml"
  } | sort -u > "$left_list"
  while IFS= read -r f; do
    [ -n "$f" ] && [ -f "$f" ] || continue
    if grep -q 'ljos-mcp\|ljos hook\|mcp_servers\.ljos' "$f" 2>/dev/null; then
      say "  $f still points at ljos. The programs are gone, so the assistant will fail to start it. Edit $f and delete the ljos entry."
      still=1
    fi
  done < "$left_list"
  rm -f "$left_list"
fi
if [ "$still" = 1 ]; then
  say "Next: edit each file named above and delete the ljos entry. Then restart the assistant."
fi
say "Done. A running agent session keeps its old hook list until it restarts; the stub answers it."
if [ "$still" = 1 ]; then
  exit 1
fi
