`ljos upgrade` installs each seat program from its own release.
One line names the old version and the new one, or `current`.
A missing release or a bad checksum is a warning.
The other programs still move.
`--only NAME` moves one program.
The old binary is kept as `NAME.bak-pre-VERSION`.
Intel macOS and aarch64 Linux use a published archive too.
