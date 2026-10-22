The prompt and failure hooks no longer lose an old memory before
they test it. The pack cuts its list by a score that decays with
age, and the hooks read only its first ten, in that order. They now
ask for a wider list, and what passes is read by how well it
matches, with the pack's score only breaking a tie.
