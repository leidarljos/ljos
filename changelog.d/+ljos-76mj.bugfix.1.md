A tracker commit refused the tracker's own checkout, and named that checkout as a product.
A checkout counts as the tracker when VISSUE_ROOT names it, when it is the seat's tracker, or when is_tracker holds.
A checkout that already holds an issue id counts as well, and a board with no id stays a product.
