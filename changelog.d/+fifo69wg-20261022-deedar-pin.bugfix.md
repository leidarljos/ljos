The cloud finish test read `accession=` from whatever `deedar` was on PATH.
A released deedar prints `id=` and no such line, so the test panicked on a machine that had the release installed.
The test now builds the deedar commit named in `.github/deedar-pin`, the same one CI installs.
