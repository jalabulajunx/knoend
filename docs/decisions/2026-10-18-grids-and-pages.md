# Key grids other than 3x3, and pages of keys

2026-10-18 - Tom (Claude Code)

## What

- **Grid per layout.** A library may set `grid = "2x2"` (cols x rows, 1-4 each) in `[pack]`; a page
  may override it (`[[page]] number = 2, grid = "4x4"`). Without one, the grid is 3x3, so every
  existing library is unchanged. `slot` runs from 1 to cols x rows, left to right, top to bottom.
- **Pages.** A key may say `page = 2` (default 1). Keys with neither `page` nor `slot` fill the next
  free slot and run onto the next page when one is full.
- **The board says how much it can draw.** Its identity's `board.grid` is now the largest grid it
  draws legibly, worked out from its screen size (at most 4x4). A page whose grid is bigger is
  split by the companion onto pages of the board's size, keys in slot order.
- **The companion switches pages.** Each page is its own `openpager.layout` packet, with `grid`,
  `page` and `pages`. The board sends `openpager.page` (`to`: `next` / `prev`) on a horizontal
  swipe or a tap on "2/3 >" at the right end of the status bar; the companion sends that
  page. `openpager.key` carries the page it was tapped on, so a tap that crosses a page change is
  dropped instead of running the wrong key.
- **Who picks the grid:** the person writing a library (recipes; later an edit mode in the web app).
  For AI drafts the companion sets it from the number of keys the model chose (1: 1x1, 2: 1x2,
  3-4: 2x2, 5-9: 3x3, 10-16: 4x4, one page) and the prompt tells the model the rule; the model does
  not place keys. Built-in recipes stay 3x3 until someone gives them a grid.
- **Fixed slots became positions in the grid:** the media row is the bottom row (Prev, Play, Next;
  only Play and Next with two columns, Play alone with one), only on grids of 3 rows or more, where it
  does not take half the keys; "Go to Claude" is the last key; a
  layout's `detail` replaces the middle rows (all but the top and bottom) and needs 3 rows or more.
  Agent screens state `grid = "3x3"`. The launcher fills 3x3 pages (or the board's grid if smaller).

## Why

- Big keys for a few actions (1x2 Allow/Deny, 2x2), more keys for apps with many shortcuts (4x4),
  and tiny screens all need the grid to vary per layout, not per board.
- Paging in the companion keeps one place (`service.rs` `show()`) deciding what the board shows,
  and keeps the firmware a renderer: the board does not store pages or know what is on them.
  Cost: a page turn is a round trip (tens of ms over Wi-Fi), and pages cannot be turned while the
  companion is away (the last page shown keeps working).
- Swipes are worked out by LVGL from the touch coordinates, which every supported touch chip
  reports the same way (FocalTech layout: FT6336, CST826, CST816; GT911), so they do not depend on a
  chip's own gesture support. The detail panel already scrolls by swiping on the ONX. The tappable
  page indicator ("2/3 >") is there because a swipe is not discoverable and touch may be poor on some panels.

## Alternatives rejected

- **Board-side paging** (send every page at once, the board flips locally): faster and works without
  the companion, but needs pages stored in firmware, larger packets (4096-byte limit) and the slot
  map duplicated on both sides. Not worth it for a prototype.
- **A "More" key** for the next page: costs a key on every page, half the keys on a 1x2.
- **Grid per board only** (the board picks the grid, libraries reflow): layouts could not ask for big
  keys, and fixed positions (media row, Allow/Deny) would move unpredictably.
- **The companion picks every grid from the key count** (also for recipes without a grid): would
  change how today's layouts look (a 4-key site suddenly in big keys). Only AI drafts do this.
- **Page buttons in the web app's dashboard:** clicking them focuses the browser, so the board
  switched to the browser's keys and the buttons turned those. Browsing and editing any app's pages
  belongs to a separate edit mode (later).
- **Positional slot names** (`row`/`col` instead of `slot`): clearer for a person, but every existing
  library and the drafting prompts use numbers; numbers within the declared grid are enough.

## Compatibility

Needs the new firmware: older firmware rejects layouts with more than 9 keys and never sends
`openpager.page`. Install the firmware from this change before testing anything after it.
