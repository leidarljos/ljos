A run's ballot no longer counts under its slot when the same pin line
was also committed on a branch whose issue had no slot and then merged
in. Before, the merge hid that branch's pin, so the slot looked older
than every pin it vouched for.
