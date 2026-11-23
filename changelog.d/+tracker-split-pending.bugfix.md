A board split now writes its split marker before it adds its issues directory to the tracker pending list.
A writer killed partway through the split no longer leaves a list that makes the next push commit issue files with no marker, and a push or flush drops such a directory from the list instead of committing it.
