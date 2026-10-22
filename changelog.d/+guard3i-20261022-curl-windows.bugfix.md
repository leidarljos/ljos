Windows curl also loads `.curlrc` or `_curlrc` from `USERPROFILE`,
`APPDATA`, and `USERPROFILE\Application Data`, and the guard did
not read those files. They are judged now in curl's order, the
Windows variables wherever they are set.
