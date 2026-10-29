`ljos onboard` no longer makes a tracker inside a checkout of something else.
A seat whose home sat in a product repository had its tracker written among that project's files.
The next `git add -A` there would commit them.
Onboard now names the checkout it found and asks for a `VISSUE_ROOT` outside it.
A path the checkout already ignores is nobody's files, and the tracker may live there.
Every habitat ljos starts is also told which tracker it means, so a ticket written from a product checkout goes to the configured tracker.
Before, a checkout holding a directory named `Software` passed for a tracker and took the write.
Standing in a tracker of your own still means that tracker.
