#!/bin/sh
# Destination allowlist a cloud seat's git runs on every push.
# Git passes the remote name and the URL it will contact, after
# insteadOf and pushInsteadOf. Refs arrive on stdin. A filter on the
# command text cannot see a push that a script, make, or another
# program starts; this hook can, because git runs it either way.

normalize() {
  u=$1
  case "$u" in
    file://*)
      local_key "${u#file://}"
      return
      ;;
  esac
  case "$u" in
    *://*)
      url_key "$u"
      return
      ;;
    *:*)
      scp_key "$u"
      return
      ;;
  esac
  local_key "$u"
}

local_key() {
  p=$1
  case "$p" in
    \~) p=${HOME:-} ;;
    \~/*) p=${HOME:-}/${p#\~/} ;;
  esac
  case "$p" in
    /*) ;;
    *) p=$(pwd)/$p ;;
  esac
  if command -v readlink >/dev/null 2>&1; then
    c=$(readlink -f -- "$p" 2>/dev/null || true)
    if [ -n "$c" ]; then
      p=$c
    fi
  fi
  while [ "$p" != "${p%/}" ]; do
    p=${p%/}
  done
  case "$p" in
    *.git) p=${p%.git} ;;
  esac
  printf '%s\n' "$p"
}

strip_slashes() {
  s=$1
  while [ "$s" != "${s%/}" ]; do
    s=${s%/}
  done
  printf '%s' "$s"
}

url_key() {
  rest=${1#*://}
  case "$rest" in
    *@*) rest=${rest#*@} ;;
  esac
  host=${rest%%/*}
  case "$rest" in
    */*) path=${rest#*/} ;;
    *) path= ;;
  esac
  host_path "$host" "$path"
}

scp_key() {
  left=${1%%:*}
  path=${1#*:}
  case "$left" in
    *@*) host=${left#*@} ;;
    *) host=$left ;;
  esac
  case "$path" in
    /*)
      local_key "$path"
      return
      ;;
  esac
  host_path "$host" "$path"
}

host_path() {
  host=$1
  path=$2
  case "$host" in
    *:*) host=${host%%:*} ;;
  esac
  path=$(strip_slashes "$path")
  case "$path" in
    *.git) path=${path%.git} ;;
  esac
  if [ -n "$path" ]; then
    printf '%s/%s\n' "$host" "$path" | tr '[:upper:]' '[:lower:]'
  else
    printf '%s\n' "$host" | tr '[:upper:]' '[:lower:]'
  fi
}

if [ "${LJOS_HOOK_KEY-}" = 1 ]; then
  normalize "${1-}"
  exit 0
fi

set -eu
z=0000000000000000000000000000000000000000
base=${HOME:-}/.config
case ${XDG_CONFIG_HOME-} in
  /*) base=$XDG_CONFIG_HOME ;;
esac
allow=$base/ljos/push-allow
if [ ! -f "$allow" ]; then
  echo "pre-push: no push allowlist; a cloud seat may push only its named branches and develop" >&2
  exit 1
fi
key=$(normalize "${2-}")
kind=
closed=0
# A code line wins over a tracker line for the same repository, so the
# tracker's wider branch list does not apply to the seat's own repo.
while IFS='	' read -r tag val; do
  case "$tag" in
    ""|\#*) continue ;;
  esac
  case "$tag" in
    code)
      if [ "$val" = "$key" ]; then
        kind=code
      fi
      ;;
    tracker)
      if [ "$val" = "$key" ] && [ "$kind" != code ]; then
        kind=tracker
      fi
      ;;
    closed) closed=1 ;;
  esac
done < "$allow"
if [ -z "$kind" ]; then
  echo "pre-push: ${2-} is not a repository this seat may push" >&2
  exit 1
fi
if [ "$kind" = code ] && [ "$closed" -eq 1 ]; then
  echo "pre-push: the seat config could not be read; a cloud seat may push only its named branches and develop" >&2
  exit 1
fi
while IFS=' ' read -r local_ref local_sha remote_ref remote_sha _; do
  if [ -z "${local_ref:-}" ]; then
    continue
  fi
  if [ "$local_sha" = "$z" ]; then
    echo "pre-push: deleting $remote_ref is refused" >&2
    exit 1
  fi
  case "$remote_ref" in
    refs/heads/*) branch=${remote_ref#refs/heads/} ;;
    *)
      echo "pre-push: $remote_ref is not a branch this seat may update" >&2
      exit 1
      ;;
  esac
  if [ "$remote_sha" != "$z" ]; then
    if ! git merge-base --is-ancestor "$remote_sha" "$local_sha" >/dev/null 2>&1; then
      echo "pre-push: $branch is not a fast-forward" >&2
      exit 1
    fi
  fi
  if [ "$kind" = code ]; then
    found=0
    while IFS='	' read -r tag val; do
      if [ "$tag" = branch ] && [ "$val" = "$branch" ]; then
        found=1
      fi
    done < "$allow"
    if [ "$found" -ne 1 ]; then
      echo "pre-push: this push updates $branch; a cloud seat may push only its named branches and develop" >&2
      exit 1
    fi
  fi
done
exit 0
