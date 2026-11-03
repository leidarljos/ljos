Tests that start packsetd use one helper, with a temporary home and a port the writer binds itself.
The writer stops when the test drops it, panics, or is signalled, so a run leaves no packsetd on the default port.
A test build does not run packset ensure or packset start, so those commands do not leave a writer on that port.
