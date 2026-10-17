# The two panels of panels.tape in the theme mc-classic, with the VGA palette that Midnight
# Commander's blue panels were made for.
#
#   just demo        (or: vhs docs/demo/panels-mc-classic.tape, after docs/demo/setup.sh)

Set Shell "bash"
Set FontFamily "SauceCodePro Nerd Font Mono"
Set FontSize 16
Set Width 1280
Set Height 720
Set Padding 16
Set WindowBar Colorful
Set TypingSpeed 60ms
Set Theme { "name": "VGA", "background": "#000000", "foreground": "#AAAAAA", "cursor": "#AAAAAA", "selection": "#555555", "black": "#000000", "red": "#AA0000", "green": "#00AA00", "yellow": "#AA5500", "blue": "#0000AA", "magenta": "#AA00AA", "cyan": "#00AAAA", "white": "#AAAAAA", "brightBlack": "#555555", "brightRed": "#FF5555", "brightGreen": "#55FF55", "brightYellow": "#FFFF55", "brightBlue": "#5555FF", "brightMagenta": "#FF55FF", "brightCyan": "#55FFFF", "brightWhite": "#FFFFFF" }

Hide
Type "exec /tmp/noc-demo/bin/noc --config /tmp/noc-demo/home/.config/noc/mc-classic.toml"
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
Screenshot assets/screenshots/panels-mc-classic.png

# Quit with Z Z, so that noc closes its connection to prod-web: there is no F10 to quit with.
Hide
Type "ZZ"
Sleep 1s
