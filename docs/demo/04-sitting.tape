# A ticket, a sitting, a note, two seats voting, the settle, and the
# recall. inky and ljosbot are two seats. One shell does not cast a
# second ballot, so a hidden Ctrl-G reads the new ticket id and arms
# `ljos`: ljosbot's vote is started by `seat` (not a shell) and the
# typed line is `ljos vote`. The lesson on the recall is the one
# 02-memory.tape stored in this HOME.
#
# Prepare the HOME the same way as 02-memory.tape, and record that
# tape first. Then:
#
#   mkdir -p "$HOME/tracker" "$HOME/notes" "$HOME/.local/bin"
#   printf 'prefix = "Software"\n' > "$HOME/tracker/vissue.toml"
#   git -C "$HOME/tracker" init
#   cp "$LJOS_SRC/docs/demo/fixtures/other-seat" "$HOME/.local/bin/seat"
#   chmod +x "$HOME/.local/bin/seat"
#
# Export VISSUE_ROOT="$HOME/tracker" and LJOS_TRACKER_GIT=off.
# PATH must already contain ljos and seat.
# Put docs/demo/bin first on PATH so bash breaks a long line on a
# space. The frame is 1280x720, about 101 columns. A long report
# is narrowed with a filter typed in the command, and the command
# stays above what it prints.

Output docs/demo/04-sitting.gif
Output docs/demo/04-sitting.mp4

Set Shell bash
Set FontFamily "Cascadia Mono"
Set FontSize 20
Set Width 1280
Set Height 720
Set Padding 16
Set Framerate 10
Set TypingSpeed 2ms
Set WaitTimeout 20s
Set Theme {"name":"paper","background":"#f6f2e8","foreground":"#15130f","cursor":"#7f520b","selection":"#e7e0d0","black":"#15130f","red":"#8c3a2f","green":"#3f5c45","yellow":"#7f520b","blue":"#3d4f66","purple":"#6b4c62","cyan":"#3d5c58","white":"#f4f0e6","brightBlack":"#5c564b","brightRed":"#a34b36","brightGreen":"#4e7348","brightYellow":"#b07a22","brightBlue":"#4d6480","brightPurple":"#7d5a74","brightCyan":"#4e736e","brightWhite":"#f6f2e8"}

Sleep 100ms
Type "ljos file --top -p demo -t decision --options ship,hold 'Ship the paper palette'"
Sleep 20ms
Enter
Wait+Screen /TODO/
Sleep 3.5s
Hide
Ctrl+G
Sleep 200ms
Show
Type "ljos sitting $id | grep -E 'remembers|lesson|is yours'"
Sleep 20ms
Enter
Wait+Screen /is yours/
Sleep 4.3s
Type "ljos note $id 'Cream paper and ink type, one gold accent.'"
Sleep 20ms
Enter
Wait+Screen /noted in memory/
Sleep 3.5s
Type "LJOS_SEAT=inky ljos vote $id --for ship --used none | head -6"
Sleep 20ms
Enter
Wait+Screen /ballot signed by inky/
Sleep 3.5s
Type "LJOS_SEAT=ljosbot ljos vote $id --for ship --used none | head -6"
Sleep 20ms
Enter
Wait+Screen /ballot signed by ljosbot/
Sleep 3.8s
Type "ljos consensus $id --no-file | grep -B1 -A3 '^settle '"
Sleep 20ms
Enter
Sleep 400ms
Sleep 3.5s
Type "ljos recall $id | grep -A4 '^Memory$'"
Sleep 20ms
Enter
Sleep 400ms
Sleep 4.4s
