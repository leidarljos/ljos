The flush of held tracker notes keeps the tracker lock from the batch read through the commit, then reads the batch again.
A note another writer held while that commit released the lock is committed too, including by the flush a process runs at exit.
