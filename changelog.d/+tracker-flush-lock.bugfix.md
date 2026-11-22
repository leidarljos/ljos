The flush of held tracker notes reads the batch record under the tracker lock it commits under, and skips a file whose held notes another seat already committed, instead of committing it with an empty subject.
The flush a process runs at exit prints on stderr what did not commit.
