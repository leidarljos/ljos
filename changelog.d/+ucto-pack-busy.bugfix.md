`ljos search` retries a busy packsetd for about two seconds, so it still answers by meaning while packsetd serves another seat.
When that wait runs out, one line says word search was used because memory search was busy.
A tracker write does not spend that wait.
