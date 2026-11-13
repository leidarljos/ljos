`ljos doctor` prints the seat source row in every case, including a named seat and a recent write.
A read that matched no rule still says it uses every seat.
A write that matched no rule names the `unknown-<host>` seat it fell back to.
