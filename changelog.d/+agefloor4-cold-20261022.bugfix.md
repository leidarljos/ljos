A prompt sent while the encoder is still loading ranks the stored
memories as the other hooks do. It scored a rare word by raw BM25,
which could pass 1, and a memory that named the prompt's subject at
a flat 1, so one word repeated in a memory about something else
outranked the lesson that covered the prompt. It now reads the
graded match, where naming more of the prompt scores higher and a
rare word alone stays below naming the subject.
