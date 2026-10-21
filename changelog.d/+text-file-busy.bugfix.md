`mcp_forward` retries a spawn that fails with "Text file busy" for about a second, and the tests close and fsync a script before they run it.
