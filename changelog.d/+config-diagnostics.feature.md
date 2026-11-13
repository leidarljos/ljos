A typo, a key in the wrong section, or a value of the wrong type produces a warning.
So does a missing or unreadable path, a file that does not parse, an unknown `LJOS_` variable, or a project file that shadows the home file.
Invalid config text names the file, the line and the column.
An unknown key or `LJOS_` name names the nearest valid one, and a wrong type names the key and the expected type.
`ljos doctor` names the file or the environment variable that set each value.
A hook and the model-context-protocol server print the warning and keep going.
