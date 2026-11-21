The finish tests keep the inherited `PATH` behind their stubs, so `age` from Homebrew is found, and each test that needs a sealed log asks for `age-keygen` first.
