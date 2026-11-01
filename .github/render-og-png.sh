#!/usr/bin/env bash
# Draw docs/source/_static/og.png from og.svg at 1200 by 630.
# Pin: resvg 0.48.1
# https://github.com/linebender/resvg/releases/download/v0.48.1/resvg-linux-x86_64.tar.gz
# sha256 fa8c26495a187e592c501db15bf9e8a9fdc051d4b2b336b39703d5b59f912b9d
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
svg="${root}/docs/source/_static/og.svg"
png="${root}/docs/source/_static/og.png"

version="0.48.1"
url="https://github.com/linebender/resvg/releases/download/v${version}/resvg-linux-x86_64.tar.gz"
sha="fa8c26495a187e592c501db15bf9e8a9fdc051d4b2b336b39703d5b59f912b9d"
cache="${XDG_CACHE_HOME:-${HOME}/.cache}/resvg-${version}"
mkdir -p "${cache}"
if [[ ! -x "${cache}/resvg" ]]; then
  tmp="$(mktemp)"
  curl -fsSL -o "${tmp}" "${url}"
  echo "${sha}  ${tmp}" | sha256sum -c -
  tar -C "${cache}" -xzf "${tmp}"
  rm -f "${tmp}"
fi

# Gelasio's metrics match Georgia, and the card asks for that family.
# The runner does not ship Georgia. The pinned file stays unmodified;
# a cache copy renames the family so resvg's match hits it.
# Pin: Gelasio variable, google/fonts 90eb9d28b0fe86d0488a49b25cb219caea7f5222
# https://github.com/google/fonts/raw/90eb9d28b0fe86d0488a49b25cb219caea7f5222/ofl/gelasio/Gelasio%5Bwght%5D.ttf
# sha256 4daecea457258c9ebeb8bc99ed3fd24353618bfad3ea4b93fa0b5d0468fc04e4
font_url="https://github.com/google/fonts/raw/90eb9d28b0fe86d0488a49b25cb219caea7f5222/ofl/gelasio/Gelasio%5Bwght%5D.ttf"
font_sha="4daecea457258c9ebeb8bc99ed3fd24353618bfad3ea4b93fa0b5d0468fc04e4"
font_cache="${XDG_CACHE_HOME:-${HOME}/.cache}/gelasio-${font_sha}"
pinned="${font_cache}/Gelasio-wght.ttf"
renamed="${font_cache}/Georgia.ttf"
mkdir -p "${font_cache}"
if [[ ! -f "${pinned}" ]]; then
  tmp="$(mktemp)"
  curl -fsSL -o "${tmp}" "${font_url}"
  echo "${font_sha}  ${tmp}" | sha256sum -c -
  mv "${tmp}" "${pinned}"
fi
if [[ ! -f "${renamed}" ]]; then
  python3 - "${pinned}" "${renamed}" <<'PY'
import sys

src, dst = sys.argv[1], sys.argv[2]
data = bytearray(open(src, "rb").read())
count = int.from_bytes(data[4:6], "big")
name_off = None
for i in range(count):
    at = 12 + i * 16
    if data[at : at + 4] == b"name":
        name_off = int.from_bytes(data[at + 8 : at + 12], "big")
        name_len = int.from_bytes(data[at + 12 : at + 16], "big")
        break
if name_off is None:
    raise SystemExit("gelasio: no name table")
name = data[name_off : name_off + name_len]
records = int.from_bytes(name[2:4], "big")
string_off = int.from_bytes(name[4:6], "big")
# Family, unique name, full name, PostScript name, and the variations prefix.
# Lengths match, so the table stays put. Copyright and the trademark line stay.
wanted = {1, 3, 4, 6, 16, 25}
for i in range(records):
    at = 6 + i * 12
    platform, _enc, _lang, name_id, length, offset = (
        int.from_bytes(name[at + j : at + j + 2], "big") for j in range(0, 12, 2)
    )
    if name_id not in wanted or platform != 3:
        continue
    start = name_off + string_off + offset
    raw = bytes(data[start : start + length])
    text = raw.decode("utf-16-be")
    renamed_text = text.replace("Gelasio", "Georgia")
    if renamed_text == text:
        continue
    encoded = renamed_text.encode("utf-16-be")
    if len(encoded) != len(raw):
        raise SystemExit(f"gelasio: name {name_id} changed length")
    data[start : start + length] = encoded
open(dst, "wb").write(data)
PY
fi

"${cache}/resvg" --version
log="$(mktemp)"
"${cache}/resvg" -w 1200 -h 630 \
  --skip-system-fonts \
  --use-font-file "${renamed}" \
  --serif-family Georgia \
  "${svg}" "${png}" 2>"${log}" || {
  cat "${log}" >&2
  rm -f "${log}"
  exit 1
}
if grep -q "No match" "${log}"; then
  cat "${log}" >&2
  rm -f "${log}"
  echo "render-og-png: Georgia did not match" >&2
  exit 1
fi
rm -f "${log}"
python3 - "${png}" <<'PY'
import sys

data = open(sys.argv[1], "rb").read()
if data[1:4] != b"PNG":
    raise SystemExit("og.png: not a PNG")
start = data.index(b"IHDR")
width = int.from_bytes(data[start + 4 : start + 8], "big")
height = int.from_bytes(data[start + 8 : start + 12], "big")
if (width, height) != (1200, 630):
    raise SystemExit(f"og.png: got {width}x{height}, wanted 1200x630")
print(f"render-og-png: wrote {sys.argv[1]} ({width}x{height})")
PY
