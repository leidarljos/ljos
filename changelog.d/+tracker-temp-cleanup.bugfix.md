A tracker file write that fails or panics no longer leaves a `.NAME.tmp.*` file beside the board.
The first tracker write in a process removes such files in the board's directory and its `issues/` directory once they are an hour old.
