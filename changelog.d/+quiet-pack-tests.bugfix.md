A unit test's silent pack no longer makes another thread queue its lesson.
Each test thread reads its own config directory, so one rewrite does not hold the others.
