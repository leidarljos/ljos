A tool-call record is kept under the home cache as well as `$XDG_RUNTIME_DIR/ljos`.
A later ljos whose `XDG_RUNTIME_DIR` differs from the hook that wrote the record still finds it.
