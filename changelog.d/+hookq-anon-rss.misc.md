The hook burst test bounds anonymous resident memory at the stdin read and how much that grows afterwards, instead of the process peak resident set.
The peak is mostly the debug executable's file pages, and those move with machine load.
Under load the calls held 1684-1692 KB at the read, and the busiest call in a run grew at most 488 KB. The caps are 4096 KB and 1024 KB.
A sample is not a high-water mark. A spike freed between two reads is invisible, and a call with no sample of the live child after stdin is closed fails.
