## The viewer, F3. Prefix: viewer-. As in mc.

# Right of the title: the first line on screen, the lines, and how far the last one on screen
# is.
viewer-position = { $line }/{ $lines } { $percent }%
# The same, once the end of the text shows.
viewer-position-end = { $line }/{ $lines } End
# The same, before the whole file was read: its lines are not known yet, and how far is in
# bytes.
viewer-position-part = { $line }/? { $percent }%
# The same after a jump, as to the end of a large file with G: the lines before were not
# counted, so the first line on screen is at about so many bytes, short, with their unit, of
# the size of the file.
viewer-position-bytes = { $at }/{ $size } { $percent }%
# The same, once the end of the text shows.
viewer-position-bytes-end = { $at }/{ $size } End
# In the middle of the dimmed line across the screen after the last line of the text.
viewer-end = EOF
# The same, where the file goes on past the parts read: scrolling on reads the next one.
viewer-end-more = more below
