The packsetd test helper kills a writer before it clears that writer's node.
A SIGINT between those two steps can no longer leave the writer running.
