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

# The right panel goes to prod-web through quick cd. Alt-C is typed as Esc, then c: the
# browser behind vhs turns Alt+c into a letter of its own.
Tab
Escape
Type "c"
Sleep 300ms
Type "prod-web:"
Enter
Sleep 2s
Down
Down
Tab
Down
Down
Down
Sleep 500ms
Screenshot assets/screenshots/panels.png

# Quit with F10, as Esc, then 0, so that noc closes its connection to prod-web.
Hide
Escape
Type "0"
Sleep 1s
