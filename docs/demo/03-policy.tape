# The command check, with no model in the loop. A recursive delete
# outside /tmp is refused with the reason the guard prints. The check
# runs nothing.
#
# Prepare the HOME the same way as 02-memory.tape, and
# `mkdir -p "$HOME/notes"`. PATH must already contain ljos.
# Put docs/demo/bin first on PATH so bash breaks a long line on a
# space. The frame is 1280x720, about 101 columns. The refusal
# stays under the command that asked.

Output docs/demo/03-policy.gif
Output docs/demo/03-policy.mp4

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
Type "ljos policy -- rm -rf $HOME/notes"
Sleep 20ms
Enter
Wait+Screen /rm-rf-outside-tmp/
Sleep 4.8s
