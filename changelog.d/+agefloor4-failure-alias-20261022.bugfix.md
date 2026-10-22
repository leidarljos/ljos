A failed command recalls a lesson that names its tool by another name
in the same family. The two words a lesson has to share with the
failure were counted without aliases, so a lesson on `pnpm` and the
registry shared one word with an `npm` registry error. Aliases now
count, and a word in several alias groups still counts once.
