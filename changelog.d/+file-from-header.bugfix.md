`ljos file --from` refuses a CSV whose header names a column twice once spaces are trimmed, such as `title` and `title `, naming both columns, instead of keeping the second one's values.
