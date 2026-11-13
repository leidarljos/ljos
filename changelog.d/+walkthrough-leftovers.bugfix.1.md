`ljos mesh up` and `ljos mesh status` exit non-zero when they cannot do what was asked.
`ljos mesh status` names `ljos mesh up` when tailscale is not on PATH.
`ljos mesh down` still exits 0 when the machine is not on the mesh.
