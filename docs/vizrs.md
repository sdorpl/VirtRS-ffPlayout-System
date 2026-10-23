# VizRS Graphics

ffplayout can drive a [VizRS](https://github.com/sdorpl/VizRS-CG-System) graphics
server: when a clip starts, the channel sends VizRS commands over its TCP port
(default `6100`). The same server can be operated by hand in VizRS Trio; its
`/output` page carries the graphics.

Enable it in **Settings → General → VizRS graphics**:

| Field | Meaning |
|---|---|
| Address | `host:port` of the VizRS TCP port, e.g. `127.0.0.1:6100` |
| Token | VizRS `--token`; sent as `AUTH <token>` first. Empty when VizRS runs without a token. |
| Clip | commands sent when a regular clip starts |
| Ad | commands sent when a commercial (`"category": "advertisement"`) starts |
| Burn-in | VizRS `/output` page burned into the program video, e.g. `http://127.0.0.1:8088/output`; empty = off |

One command per line, using the VizRS TCP syntax (`TAKE`, `UPDATE`, `CONTINUE`,
`OUT`, `CLEAR`, `MACRO`, `TICKER`, ...). Empty lines and lines starting with `#`
are ignored.

Placeholders:

| Placeholder | Value |
|---|---|
| `{title}` | clip title, or the file name without extension |
| `{source}` | clip path or URL |
| `{duration}` | played length, `hh:mm:ss` |
| `{next_title}` | title of the next clip |
| `{next_start}` / `{next_start_full}` | start of the next clip, `hh:mm` / `hh:mm:ss` |
| `{next_duration}` | length of the next clip, `hh:mm:ss` |

`|` and line breaks in values are replaced, so they cannot break the
`key=value|key=value` syntax.

Example – "now / next" strap and logo off during ads:

```
TAKE 4001 headline={title}|sub=Next: {next_title} at {next_start}
```

```
OUT main
```

## Commands per clip

A playlist item can carry its own commands in a `vizrs` list. They are sent
after the channel commands:

```json
{
    "in": 0,
    "out": 1800,
    "duration": 1800,
    "source": "/media/news.mp4",
    "title": "News",
    "vizrs": ["TAKE 2001 place=Warsaw|live=LIVE"]
}
```

## Burn-in

With **Burn-in** set, the channel starts a headless Chromium (no GPU) on that
page when playout starts, with the browser viewport set to the channel size
(for example 1280x720). VizRS keeps its 1920x1080 stage and scales it to the
viewport, so do not add `?w=&h=` to the address. Other `/output` parameters
such as `?layers=main,front` work as usual.

Chromium sends a PNG frame with alpha (DevTools screencast) only when the page
changes. ffplayout converts it once, crops it to the visible area and blends it
over every video frame after logo and text, also over fillers and generated
black. Static graphics therefore cost almost nothing; animations cost one PNG
decode per changed frame.

The browser is searched in `PATH` (`chromium`, `chromium-browser`,
`google-chrome`, `google-chrome-stable`, `chrome`, `headless_shell`). Set
`VIZRS_BROWSER` to use another binary. In containers running as root set
`VIZRS_BROWSER_NO_SANDBOX=1`, because Chromium refuses its sandbox there.

If the browser crashes or VizRS is not reachable, the graphics are cleared and
the browser is restarted after 3 seconds; playout continues.
Changing the setting requires a channel restart.

## Failure handling

Graphics never stop the playout. Commands are sent in the background, with a
2 second limit for connecting and for each reply. Errors and `ERR` replies are
written to the channel log; successful replies are logged at debug level.
