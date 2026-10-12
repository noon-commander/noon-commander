# Two panels: a local project on the left, a host over SFTP on the right.
#
#   just demo        (or: vhs docs/demo/panels.tape, after docs/demo/setup.sh)

Set Shell "bash"
Set FontFamily "SauceCodePro Nerd Font Mono"
Set FontSize 16
Set Width 1280
Set Height 720
Set Padding 16
Set WindowBar Colorful
Set TypingSpeed 60ms

Hide
Type "exec /tmp/noc-demo/bin/noc"
Enter
Sleep 2s
Show

# The right panel goes to prod-web through its location menu, Ctrl+w 2, whose filter / types
# in: the browser behind vhs sends neither Alt nor F-keys, so Alt+F2 does not reach noc.
Ctrl+W
Type "w"
Ctrl+W
Type "2"
Sleep 300ms
Type "/prod-web"
Enter
Sleep 2s
Down
Down
Ctrl+W
Type "w"
Down
Down
Down
Sleep 500ms
Screenshot assets/screenshots/panels.png

# Quit, so that noc closes its connection to prod-web. There is no F10 to quit with, but noc
# quits on SIGTERM too; the command runs in the left panel, whose shell noc started.
Hide
Type "!kill -TERM $PPID"
Enter
Sleep 1s
Enter
Sleep 1s
