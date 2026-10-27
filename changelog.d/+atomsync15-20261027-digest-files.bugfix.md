A tracker log that a merge or a checkout replaced kept its digest, so
ljos sync left the other seal in place while the atoms matched. The
digest now names the sealed file too. The first sync after upgrading
seals each log once, since older digests do not name it.
