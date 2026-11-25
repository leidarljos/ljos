A seat with no absolute `HOME` is told its temporary state directory is removed when the command exits normally.
A signal, `exec` or abort skips that removal, so the warning no longer says the state will be discarded.
