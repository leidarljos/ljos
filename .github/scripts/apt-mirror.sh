#!/bin/sh
# If the runner cannot resolve the default Ubuntu archive, point apt at
# a mirror that does, including the security pocket, then update.
set -eu

probe=azure.archive.ubuntu.com

resolved() {
  command -v getent >/dev/null 2>&1 && getent hosts "$1" >/dev/null 2>&1
}

write_sources() {
  uri=$1
  . /etc/os-release
  suite=${VERSION_CODENAME:-}
  if [ -z "$suite" ]; then
    echo "apt-mirror: /etc/os-release has no VERSION_CODENAME" >&2
    exit 1
  fi
  key=/usr/share/keyrings/ubuntu-archive-keyring.gpg
  deb822=/etc/apt/sources.list.d/ubuntu.sources
  list=/etc/apt/sources.list

  if [ -e "$deb822" ]; then
    sudo tee "$deb822" >/dev/null <<SRC
Types: deb
URIs: $uri
Suites: $suite $suite-updates $suite-backports
Components: main restricted universe multiverse
Signed-By: $key

Types: deb
URIs: $uri
Suites: $suite-security
Components: main restricted universe multiverse
Signed-By: $key
SRC
    if [ -e "$list" ]; then
      sudo tee "$list" >/dev/null <<SRC
# Ubuntu packages come from /etc/apt/sources.list.d/ubuntu.sources
SRC
    fi
  else
    sudo tee "$list" >/dev/null <<SRC
deb $uri $suite main restricted universe multiverse
deb $uri $suite-updates main restricted universe multiverse
deb $uri $suite-backports main restricted universe multiverse
deb $uri $suite-security main restricted universe multiverse
SRC
  fi
}

if resolved "$probe"; then
  echo "apt-mirror: $probe resolved, leaving sources as they are"
else
  echo "apt-mirror: $probe did not resolve"
  uri=
  for item in \
    mirrors.edge.kernel.org/ubuntu \
    mirror.math.princeton.edu/pub/ubuntu \
    ftp.osuosl.org/pub/ubuntu
  do
    host=${item%%/*}
    if resolved "$host"; then
      uri="http://$item"
      echo "apt-mirror: $host resolved, using $uri"
      break
    fi
    echo "apt-mirror: $host did not resolve"
  done
  if [ -z "$uri" ]; then
    echo "apt-mirror: no fallback mirror resolved" >&2
    exit 1
  fi
  write_sources "$uri"
fi

sudo apt-get update -o Acquire::Retries=5
