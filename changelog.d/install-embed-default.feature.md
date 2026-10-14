`install.sh` installs `packset-embed`, the encoder, by default.
`--no-embed` leaves it out, `--with-embed` is still accepted, and a platform with no encoder build gets a warning instead of a failed install.
On the 20-memory set the default install now delivers 20 of 20 paraphrases, against 7 of 20 without the encoder.
The installer also fetches the encoder's model, so the first search is not lexical; a failed fetch is a warning.
