`ljos mesh pull --seat SEAT --from DIR` imports lane JSONL files into a seat, skips duplicate texts, and writes a stable seed.
`ljos mesh push --seat SEAT --to DIR` writes that seat's memories to the handoff as one lane file.
A pull or a push holds a file lock, tags each lesson with its source lane, and retries a pack that answers 503 or stops on 408.
