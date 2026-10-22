The cloud VM doc no longer says `cloud up` builds `packset-embed` from source off a cloud VM.
Off a cloud VM the command downloads no encoder. `sh scripts/install.sh --cloud` sets `WITH_EMBED=0`.
