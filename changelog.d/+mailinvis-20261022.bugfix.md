The mail checks fold away, and export refuses, every Unicode
Default_Ignorable_Code_Point character rather than a hand-kept list, so a
denied term split by a variation selector (U+FE00-FE0F, U+E0100-E01EF) or
a tag character (U+E0000-E007F) is caught. Shown mail replaces them with
U+FFFD.
