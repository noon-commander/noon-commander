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

# The right panel goes to prod-web through its location menu, Ctrl+w 2, whose filter takes what
# is typed: the browser behind vhs sends neither Alt nor F-keys, so Alt+F2 does not reach noc.
# Choosing a place makes the right panel active.
Ctrl+W
Type "2"
Sleep 300ms
Type "prod-web"
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

# Quit with Z Z, so that noc closes its connection to prod-web: there is no F10 to quit with.
Hide
Type "ZZ"
Sleep 1s
