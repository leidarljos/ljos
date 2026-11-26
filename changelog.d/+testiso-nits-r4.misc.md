A move closure or async move block ends its guard when that name is any token in the body, including inside a macro.
A leading let of the same name still leaves the outer guard in place.
