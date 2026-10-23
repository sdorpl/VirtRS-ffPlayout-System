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

## Failure handling

Graphics never stop the playout. Commands are sent in the background, with a
2 second limit for connecting and for each reply. Errors and `ERR` replies are
written to the channel log; successful replies are logged at debug level.
