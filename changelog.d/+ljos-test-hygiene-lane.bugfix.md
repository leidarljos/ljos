Tests stop depending on a fixed home path in `cloud_hook` fixtures, and `cwd_probe` now covers gone and unreadable working directories so those probe cases return without waiting.
