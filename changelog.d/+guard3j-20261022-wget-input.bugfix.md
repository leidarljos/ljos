`wget -i FILE` fetched every URL in the file and `wget --base=URL`
resolved a relative one against GitHub, while the guard judged only
the URL on the command line. An input file, a base, and a `Host:`
header on the wget command line now leave the target unknown, split
or attached, in a short cluster, or abbreviated, and the command line
is read after a wgetrc the guard found opaque.
