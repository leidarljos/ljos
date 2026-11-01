# A lesson typed at the terminal, then a reworded question that brings
# it back. Session-end extraction is on by default, and the pack refuses
# the note when it names no ticket, so this picture does not run that
# step. The sentences in docs/demo/fixtures/ are the input that refusal
# answers; they are not played here.
#
# The same HOME is scene 4, so the recall there shows this lesson.
# HOME must sit outside /tmp. PATH must already contain ljos.
# Put docs/demo/bin first on PATH so bash breaks a long line on a
# space. The frame is 1280x720, about 101 columns. The hook
# command stays above the lesson it recalls.

Output docs/demo/02-memory.gif
Output docs/demo/02-memory.mp4

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
Type "ljos remember 'Ship the paper palette with cream paper and ink type.'"
Sleep 20ms
Enter
Wait+Screen /standing:/
Sleep 3.5s
Type "echo 'Should the paper palette ship with cream paper?' | ljos hook --prompt --text"
Sleep 20ms
Enter
Wait+Screen /lesson, today/
Sleep 4.8s
