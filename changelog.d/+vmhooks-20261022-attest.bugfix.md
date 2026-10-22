A release with no build attestation warns `no attestation published` and names the reason.
`gh attestation verify` used to hide "no attestations found" on stdout, so the warning was a blank "does not verify".
An HTTP 404 from the attestation API is the same warning, and `--attest` still stops the install.
