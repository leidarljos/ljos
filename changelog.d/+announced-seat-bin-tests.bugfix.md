The `ljos-mcp` unit tests now end an announced seat when their process-state guard drops, as the library's tests do, so one server test cannot leave its seat to the next.
Release builds keep the one-time announcement.
