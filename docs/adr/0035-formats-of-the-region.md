# 0035. Formats of the region

- Status: Accepted
- Date: 2026-10-17

## Context

Noon Commander wrote dates, times, and numbers one way for everyone: months by their English
names (`Oct 17 12:34`), a comma between thousands (`1,234,567`), a point before decimals
(`1.5M`). Only the language of the messages followed the system (`ui.language`, Fluent). A user
whose region writes `17.10.2026` and `1 234,5` saw neither, and a translation of the messages
would not have changed them either.

Fluent's numbers did not help. i18n-embed's bundles have no `NUMBER()`, and Fluent writes a
number as Rust does; numbers were passed to messages as strings already formatted, so the plural
forms of a translation could not see them.

Where the region comes from differs by system. POSIX gives each part its variable: `LC_TIME`,
`LC_NUMERIC`, `LC_COLLATE`, with `LC_ALL` over them and `LANG` under them. macOS keeps the
region in System Settings → General → Language & Region, with the user's own date format,
separators of numbers, and 24-hour time on top of it: the identifier of the locale (`en_US`)
does not show them, and terminals set `LANG` from the language, not from the region. Only
Foundation's formatters for the current locale apply them all.

The order of names had the same gap: panels sorted by characters in lower case, so `file10`
came before `file2`, and letters with accents after `z`.

## Decision

- **A crate of its own, `noc-locale`**, depends on no other `noc-*` crate. Its `Formats` write
  numbers (grouped by thousands, with a fraction), times as a date with its year and a time of
  day, and compare names. Library code
  takes `&Formats`; the formats in use are global, as the language of the messages is, so that
  numbers in messages and those drawn without one agree. Tests never change them and see `iso`.
- **`ui.formats`: `auto` (default), `iso`, or a language tag.** `auto` takes each part from
  `LC_ALL`, then its own variable (`LC_TIME`, `LC_NUMERIC`, `LC_COLLATE`), then, on macOS, the
  settings of the system, then `LANG`. `C` and `POSIX` mean `iso`, as does a system that names
  nothing. A tag names a region's formats in CLDR. A change applies at once.
- **On macOS, the system writes them.** Dates and numbers come from `NSDateFormatter` and
  `NSNumberFormatter` of the current locale, through `objc2-foundation`, whose calls are safe:
  the user's own short date and time patterns, the year of the date's in two digits
  (`dd.MM.y` → `dd.MM.yy`). Each call runs in an autorelease pool, which Foundation needs.
- **Elsewhere, and for a tag, ICU4X writes them**, from CLDR's data compiled in: the short
  year-month-day, with two digits of the year where the region's short form has them, and
  hour-minute in the hour cycle of the region.
  Names compare by ICU4X's collator for the language of the formats, with numbers by value.
- **`iso` is neutral**: `2025-10-17 14:05`, with the whole year, which ISO 8601 needs and which
  keeps the year from reading as a day; numbers as the C locale writes them, names in the root
  order of Unicode.
- **No language or region is special in the code.** Every format comes from the system or from
  CLDR; tests use `iso` and patterns of their own, never a region's.
- **Every time shows one way: its date, with the year, and its time of day**, in the panels, as
  `12.10.26 05:05`, and in dialogs. The rule of `ls -l` and mc, the day and the time for the last
  six months and the date alone before, made a column of two shapes, which read badly.
- **Dates are written in digits only.** Names of months would mix the language of the formats
  with that of the messages; digits read the same in both.
- **Every number is grouped as the region groups it**, file sizes too. Sizes in the panels'
  column, on their frame, in the location menu, and in the window of a job have a unit and a
  decimal below 100 (`512B`, `78.5K`, `503G`), so the unit always takes the last cell of the
  column and lines up. A column of grouped bytes, then K, M, G above them, as in Far, left the
  digits of bytes one cell right of the others, and `515 123M` looked no bigger than `80 416`;
  a cell kept for the unit lined the digits up but not the sizes, and a column of every byte
  needs 15 cells for 999 GB. The status line shows every byte of the file under the cursor at
  its right, grouped, as Far's does. Numbers that name rather than count stay as they are: the
  digits of tabs and F-keys, a count typed before a command, and the numbers typed in the
  Configuration dialog.
- **The unit of a size in the column grows bolder and brighter with the size**, a role of the
  theme (`size_units`): B and K as the text, M bold, G bold in the theme's accent, T and above
  bold in red, so that big files show and the digits keep the row's colors; as
  `eza --color-scale` and `lsd` color sizes, the unit alone. Without colors, in `terminal`, M
  and above are bold. Under the cursor and on marked rows it takes the row's colors.
- **Fluent formats its numbers.** Both loaders, the app's and the viewer's, get
  `noc_locale::fluent` as the formatter of their bundles, renewed after every load as the
  isolation marks are. Messages take numbers as numbers, so plural forms work; the language of the
  messages picks the plural forms, the formats of the region write the digits.
- **Columns take what the formats need**: the time column is as wide as the longest time of the
  formats, the size column as its longest size and its words; neither is narrower than its
  title. Marks of the direction of text that some formats have are left out, as terminals show
  them as garbage.
- **`ui.natural_sort`, on by default**, sorts the names of panels with the collator; off, as
  before, by their characters in lower case.
- **Help → About** shows the language of the messages and where the formats come from, and
  `noc debug version` prints both (`language`, `formats`).

## Consequences

- Dates, sizes, speeds, counts, and percentages follow the region, on macOS exactly as the
  system shows them.
- ICU4X and its compiled data make the binary larger: the release binary for arm64 macOS grew
  from 7.5 MiB to 9.2 MiB, most of it the collator's data. macOS builds link Foundation through
  `objc2`.
- A change in System Settings shows after the next start or a change of `ui.formats`: the
  formatters keep the settings they were made with.
- Durations (`1:02:05`) and the unit letters of sizes stay as they were; the letters are in the
  messages, so a translation can change them.
- Every new number or date on screen goes through `noc-locale`, and every number in a message is
  passed as a number.
