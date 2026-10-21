`ljos record check` and ballot signature checks now refuse small-order ed25519 keys, which used to verify any message.
An entry signed with the identity point as its key no longer passes, whatever its stance.
