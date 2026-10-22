A request already written to `ljos-mcp` is parsed before stdin is
treated as closed. The client watch no longer polls that fd itself:
tokio reads it on another thread, so a hangup there can show up
after the bytes have left the pipe and before they are a message.
