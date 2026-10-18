## The viewer, F3. Prefix: viewer-. As in mc.

# Right of the title: the first line on screen, the lines, and how far the last one on screen
# is.
viewer-position = { $line }/{ $lines } { $percent }%
# The same, once the end of the text shows.
viewer-position-end = { $line }/{ $lines } End
# The same, when only the start of a long file was read: how much was read, in mebibytes.
viewer-truncated = { $position } of the first { $mebibytes } MiB
# In the middle of the dimmed line across the screen after the last line of the text.
viewer-end = EOF
# The same, when only the start of a long file was read.
viewer-end-truncated = end of the first { $mebibytes } MiB
