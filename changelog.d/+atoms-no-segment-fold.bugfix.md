A sync past 64 atom segments folded them into a new snapshot, so the whole log was sealed into the tracker again every 64 changes.
A sync now only adds a segment of the new atoms, and only `ljos sync --compact` folds.
