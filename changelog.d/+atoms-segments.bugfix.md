`ljos sync` seals a changed atom into a new segment and leaves every sealed file as it was. An unchanged plaintext writes nothing. The check is a hash of the plaintext, not the ciphertext.
