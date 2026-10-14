`ljos reap --idle 6h` lists claims with no claim or tracker commit inside the window; `--apply` releases each one and mails its seat how to pick it up or finish it.
`ljos recall`, `ljos search` and `ljos hook` append one line to `recall.log` in the ljos state dir, so an audit can tell when a seat last asked its memory.
