# Install into a fresh HOME, then the first ljos the person runs.
#
# HOME must be an empty directory outside /tmp (a HOME under /tmp keeps
# the guard and the memory writer off). LJOS_SRC is this repository.
# LJOS_FROM holds the programs.
#
# Put docs/demo/bin first on PATH so bash breaks a long line on a
# space. The frame is 1280x720, about 101 columns. The install
# command keeps its Done line, and doctor keeps the ready line.

Output docs/demo/01-install.gif
Output docs/demo/01-install.mp4

Set Shell bash
Set FontFamily "Cascadia Mono"
Set FontSize 20
Set Width 1280
Set Height 720
Set Padding 16
Set Framerate 10
Set TypingSpeed 2ms
Set WaitTimeout 45s
Set Theme {"name":"paper","background":"#f6f2e8","foreground":"#15130f","cursor":"#7f520b","selection":"#e7e0d0","black":"#15130f","red":"#8c3a2f","green":"#3f5c45","yellow":"#7f520b","blue":"#3d4f66","purple":"#6b4c62","cyan":"#3d5c58","white":"#f4f0e6","brightBlack":"#5c564b","brightRed":"#a34b36","brightGreen":"#4e7348","brightYellow":"#b07a22","brightBlue":"#4d6480","brightPurple":"#7d5a74","brightCyan":"#4e736e","brightWhite":"#f6f2e8"}

Sleep 100ms
Type "sh $LJOS_SRC/scripts/install.sh --from-dir $LJOS_FROM --no-embed --no-modify-path|grep Done"
Sleep 20ms
Enter
Wait+Screen /Done in/
Sleep 3.5s
Type `export PATH="$HOME/.local/bin:$PATH"`
Sleep 20ms
Enter
Sleep 200ms
Type "ljos doctor | head -1"
Sleep 20ms
Enter
Wait+Screen /ljos is ready/
Sleep 3.9s
