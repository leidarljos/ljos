`cloud up` drops the hyphens in a run id before it keeps 16 characters, so the seat it writes is the seat a later command resolves.
A cut that kept the hyphens ended an id such as `bc-ada4e131-7ec2-5fbf-...` on `-7ec2`, and that tail was read as a process tag.
A numeric GitHub run id has no hyphens, so it is only shortened.
