## The viewer, F3. Prefix: viewer-. As in mc.

# Right of the title: the first line on screen, the lines, and how far the last one on screen
# is.
viewer-position = { $line }/{ $lines } { $percent }%
# The same, once the end of the text shows.
viewer-position-end = { $line }/{ $lines } End
# The same, before the whole file was read: its lines are not known yet, and how far is in
# bytes.
viewer-position-part = { $line }/? { $percent }%
# In the middle of the dimmed line across the screen after the last line of the text.
viewer-end = EOF
# The same, where the file goes on past the parts read: scrolling on reads the next one.
viewer-end-more = more below
