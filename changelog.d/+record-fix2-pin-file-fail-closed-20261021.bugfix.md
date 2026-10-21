`ljos record check` now fails closed on the tracker pin file: unreadable pin
files and duplicate voter rows are hard errors, and pin mismatches report
whether the source was the pin file path or first-seen pins in that check.
