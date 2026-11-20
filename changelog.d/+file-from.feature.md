`ljos file --from FILE` files one ticket per row of a JSONL or CSV file (title, body, tags, parent, priority, type) in one tracker commit and prints the new ids one per line, so 150 rows are one commit rather than 150.
A row that cannot be filed files none of them: the error names the row and the tickets written before it are taken back.
The `ljos_file_from` MCP tool takes the rows inline or a file path.
