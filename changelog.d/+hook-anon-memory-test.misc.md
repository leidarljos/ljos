The twenty-call hook test bounds RssAnon plus RssShmem at the stdin read, and how much that grows afterwards.
The caps are 4096 KB at the read and 1024 KB of growth.
File-backed pages, including the debug executable, stay out of the figure.
A call with no reading of the live child after stdin is closed fails, as does a call that exits before that read.
