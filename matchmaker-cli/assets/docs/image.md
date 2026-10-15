# Images

The preview pane can show an image instead of text. The image is drawn with the
terminal's graphics protocol (or with half blocks where there is none), keeps its
proportions, and can be zoomed and panned while the text around it stays
scrollable.

The shipped configuration already previews files this way: its first preview
layout starts with `mm --image {1} && exit 0`, so an image file is drawn as one
and anything else falls through to the text previewer.

---

## The Image Command

```sh
mm --image PATH [OPTION...]
```

`mm --image` prints an image header for `PATH` and exits `0`. Positional
arguments after the path are written into that header, so the image is shown with
those options:

```sh
mm --image logo.svg hfit=fill:20 filter=lanczos3
```

If this build has no decoder for the file (or it is not an image at all) and
`chafa` is installed, the file is rendered by `chafa` instead and printed as
half-block text. Pass `-q` to keep the command image-or-nothing.

When neither can show the file, the exit status is `159` (see `mm --doc other`),
which makes it easy to fall back to text in a preview command:

```sh
mm --image {1} && exit 0
eza --tree {1}
```

---

## Image Headers

Any preview command can emit an image header itself. When the output starts with
the `RS` byte (`\x1e`) and an `<MM_IMAGE>` tag, matchmaker reads the image out of
it instead of showing that output as text.

The tag declares how many bytes of the output belong to the image, so everything
after those bytes is shown as ordinary text, in the same preview:

```text
\x1e<MM_IMAGE:9> hfit=contain vfit=crop filter=triangle scroll=3
/path/img
```

The declared count is the byte length of the path that follows the header line.
Options go between the tag and the newline.

Inline mode carries the image data instead of a path. The format after the `/`
selects the decoder, and the count is the payload's byte length:

```text
\x1e<MM_IMAGE/png:4>
<4 png bytes ...>
```

### Header Options

| Option         | Values                                                       | Meaning                                                                    |
| :------------- | :----------------------------------------------------------- | :------------------------------------------------------------------------- |
| `hfit`, `vfit` | `contain`, `fill`, `crop`, optionally with `:cells`          | How the width and height are fitted to the pane (see [Fitting](#fitting)). |
| `filter`       | `nearest`, `triangle`, `catmull_rom`, `gaussian`, `lanczos3` | Resampling filter used when the image has to be scaled.                    |
| `zoom`         | number, `1.0` or more                                        | Magnification of the image; `1` shows it at its fitted size.               |
| `scroll`       | `N`, `N,M` or `,M`                                           | Initial offset in cells: vertical first, horizontal second.                |
| `header_lines` | number                                                       | Lines of the text after the payload drawn above the image.                 |

Values are case-insensitive and unknown options are ignored, so a producer can
use newer options without breaking older matchmakers.

Header parsing can be turned off with `previewer.images = false`, and is
automatically off when `preview.image.protocol = "none"`; such output is then
shown as plain text.

---

## Viewing

The image starts at its configured `zoom` and `scroll`, and both are reset when
the image changes.

| Action                                                    | What it does while an image is on screen                                                                                                                                     |
| :-------------------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PreviewScroll(n)`                                        | Zooms in tenths of a factor (`n = 1` is `+0.1`), floored at `1.0` and starting from the image's own zoom. Once the image has scrolled out of the pane it scrolls text again. |
| `PreviewHScroll(n)`                                       | Pans horizontally by `n` cells; `0` resets to the image's initial pan.                                                                                                       |
| `PreviewUp(n)`, `PreviewDown(n)`                          | Move through the preview's lines: the image pans vertically, then the text below it scrolls.                                                                                 |
| `PreviewHalfPageUp`, `PreviewHalfPageDown`, `PreviewJump` | Same as for text previews.                                                                                                                                                   |

A wheel-and-keys setup for images, on top of the usual text bindings:

```toml
[binds]
"scrollup" = "PreviewScroll(-1)" # zoom out
"scrolldown" = "PreviewScroll(1)" # zoom in
"alt-left" = "PreviewHScroll(-1)"
"alt-right" = "PreviewHScroll(1)"
```

See `mm --doc binds` for the full list of actions.

---

## Configuration

```toml
[preview.image]
protocol = "auto"
horizontal_fit = "contain"
vertical_fit = "crop"
filter = "triangle"
zoom = 1.0
scroll = [0, 0]
header_lines = 0
```

- `protocol`: `auto` follows what the terminal reported; `half_blocks`, `sixel`,
  `kitty` and `iterm2` force one, and `none` disables images entirely. A forced
  protocol wins even when the terminal cannot render it, which then shows
  garbage rather than the image.
- `horizontal_fit`, `vertical_fit`: how each axis is fitted to the pane, see
  [Fitting](#fitting).
- `filter`: resampling filter.
- `zoom`: magnification of `1.0` or more.
- `scroll`: initial `[vertical, horizontal]` offset in cells.
- `header_lines`: lines after the image payload drawn above the image, instead of
  with the rest of the text. The image is fitted into the pane height left below
  them.

The same keys can be overridden per preview on the command line or in the header.

### Fitting

`contain` and `fill` name the extent they fit into in cells, `0` meaning the pane
itself: `hfit=fill:16` makes the image 16 cells wide however wide the pane is,
while plain `fill` fills the pane. A `contain` extent only shrinks, never past the
image's native size; a `fill` extent grows or shrinks to reach it. `crop` leaves
the axis unconstrained and clips whatever does not fit.

The image is always scaled by a single factor that keeps its proportions, so the
two axes interact:

| Horizontal | Vertical  | Result                                                                         |
| :--------- | :-------- | :----------------------------------------------------------------------------- |
| `fill`     | `fill`    | The larger scale wins, filling the pane and cropping the overflow.             |
| `fill`     | `contain` | The fill scale wins, unless it would make the image exceed the contained axis. |
| `fill`     | `crop`    | The fill scale wins; the other axis follows it and is cropped.                 |
| `contain`  | `contain` | The smaller scale wins, so neither axis is exceeded.                           |
| `contain`  | `crop`    | The contained axis wins and the other follows it.                              |
| `crop`     | `crop`    | Nothing is scaled: the image keeps its native pixel size and is clipped.       |

---

## Terminals

`protocol = "auto"` uses the graphics protocol the terminal reports, preferring
kitty over sixel. Terminals that report nothing still show images through half
blocks, which need no special support at all.

Kitty, sixel and iTerm2 place real pixels, so they also need the terminal's cell
size to lay an image out; matchmaker asks for it and falls back to the pixel size
of the window, and without either, images are drawn with half blocks.

Some terminals and multiplexers leave the queries unanswered — a tmux pane, for
instance — so a supported protocol may not be detected. Setting `protocol` to
`kitty`, `sixel` or `iterm2` forces it.

---

## Formats

PNG and JPEG always work, and SVG too in builds with the `image-svg` feature
(the shipped binaries). SVG is rasterized on the way in, without fonts, so text
in an SVG may be missing. GIF, BMP, ICO, TIFF and WebP are available with the
`image-all` feature:

```sh
cargo build -p matchmaker-cli --release --features image-all
```

Anything this build cannot decode is handed to `chafa` by the image command when
it is installed. Animated formats show their first frame.
