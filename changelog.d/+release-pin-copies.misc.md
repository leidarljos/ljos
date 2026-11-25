`scripts/release-install-check.sh` now fails when `crates/ljos-cli/release-pin`, the copy the published crate embeds, differs from `.github/release-pin`.
It also fails when the fallback list in `scripts/install.sh` names a different set of tools or versions.
`CONTRIBUTING.md` and the pin file now name all three places the installer release pin is written.
