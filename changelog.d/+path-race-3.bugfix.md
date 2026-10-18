A test that needs a fake binary puts that directory in front of PATH while it holds the environment lock, and puts the previous PATH back when the guard drops.
