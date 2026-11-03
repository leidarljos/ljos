A long-running ljos process, such as `ljos-mcp` or `ljos-hud`, checks which user owns the pack's port again on each search or request.
When the writer stops and another user's process takes its port, the seat's token no longer goes to that process.
It goes to the writer again once the writer is back.
