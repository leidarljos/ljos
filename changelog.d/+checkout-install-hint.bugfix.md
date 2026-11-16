`scripts/install.sh` run from a checkout names `sh scripts/install.sh --from-checkout` to build that checkout's ljos and ljos-mcp and to install the sibling tools.
The published release remains a complete install.
The installer checks one archive's build attestation while the next archive downloads, and checks a shared archive once.
