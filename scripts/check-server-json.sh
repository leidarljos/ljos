#!/usr/bin/env bash
# server.json cannot carry a comment. The version at the top, and the
# version on the cargo package, are the crate version in Cargo.toml.
# A release tag rewrites both from the tag (see publish.yml), and that
# job already refuses a tag that is not the crate version.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
crate=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [[ -z "$crate" ]]; then
  echo "Cargo.toml has no version" >&2
  exit 1
fi
read -r top pkg < <(python3 -c '
import json
s = json.load(open("server.json"))
print(s["version"], s["packages"][0]["version"])
')
if [[ "$top" != "$crate" || "$pkg" != "$crate" ]]; then
  echo "server.json version is $top and the package is $pkg; Cargo.toml is $crate" >&2
  exit 1
fi
