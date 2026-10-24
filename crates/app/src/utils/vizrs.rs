//! Secondary events for a VizRS graphics server.
//!
//! When a clip starts, the channel's command template (`on_clip`) and the
//! clip's own `vizrs` list are rendered and sent to VizRS over its line-based
//! TCP protocol (`TAKE`, `UPDATE`, `OUT`, ...). Commercials are played without
//! graphics: every layer on air is taken `OUT` first, then only the `on_ad`
//! template is sent. Every command gets one `OK ...` or `ERR ...` reply line.
//!
//! Graphics are secondary: connection problems are logged, never propagated,
//! and sending runs in a background task so playout timing is not affected.

use std::path::Path;
use std::sync::LazyLock;

use log::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

use crate::player::{controller::ChannelManager, utils::Media};
use crate::utils::config::Vizrs;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// Keeps command batches in clip order, even when one batch waits on a slow
/// server while the next clip already starts.
static SEND_ORDER: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Data available to command placeholders.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ClipContext {
    pub title: String,
    pub source: String,
    pub duration: f64,
    pub next_title: String,
    pub next_start: Option<f64>,
    pub next_duration: f64,
}

impl ClipContext {
    pub fn new(current: &Media, next: Option<&Media>) -> Self {
        Self {
            title: display_title(current),
            source: current.source.clone(),
            duration: current.out - current.seek,
            next_title: next.map(display_title).unwrap_or_default(),
            next_start: next.and_then(|n| n.begin),
            next_duration: next.map(|n| n.out - n.seek).unwrap_or_default(),
        }
    }
}

/// Title from the playlist, or the file name without extension.
fn display_title(media: &Media) -> String {
    if let Some(title) = media.title.as_ref().filter(|t| !t.trim().is_empty()) {
        return title.clone();
    }

    Path::new(&media.source)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// `hh:mm:ss` for durations, wrapping clock times past midnight.
fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;

    format!(
        "{:02}:{:02}:{:02}",
        (total / 3600) % 24,
        (total / 60) % 60,
        total % 60
    )
}

/// Values must not break the `key=value|key=value` syntax or the line protocol.
fn clean_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .map(|c| if c == '|' { '/' } else { c })
        .collect()
}

fn render_line(line: &str, ctx: &ClipContext) -> String {
    let next_start = ctx.next_start.map(clock).unwrap_or_default();
    let next_start_short = next_start.get(..5).unwrap_or_default().to_string();

    [
        ("{title}", clean_value(&ctx.title)),
        ("{source}", clean_value(&ctx.source)),
        ("{duration}", clock(ctx.duration)),
        ("{next_title}", clean_value(&ctx.next_title)),
        ("{next_start}", next_start_short),
        ("{next_start_full}", next_start),
        ("{next_duration}", clock(ctx.next_duration)),
    ]
    .iter()
    .fold(line.to_string(), |acc, (key, value)| {
        acc.replace(key, value)
    })
}

/// Commands for one clip: channel template first, then the clip's own list.
/// Ads get only the `on_ad` template, their own commands are not sent.
/// Empty lines and lines starting with `#` are ignored.
pub fn render_commands(config: &Vizrs, media: &Media, ctx: &ClipContext) -> Vec<String> {
    let (template, own): (&str, &[String]) = if media.ad {
        (&config.on_ad, &[])
    } else {
        (&config.on_clip, &media.vizrs)
    };

    template
        .lines()
        .chain(own.iter().map(String::as_str))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| render_line(line, ctx))
        .collect()
}

/// `OUT` for every layer listed in a `STATE` reply (`OK {"layers": [...]}`).
fn out_command(state_reply: &str) -> Option<String> {
    let state: serde_json::Value =
        serde_json::from_str(state_reply.strip_prefix("OK")?.trim()).ok()?;
    let layers: Vec<&str> = state
        .get("layers")?
        .as_array()?
        .iter()
        .filter_map(|layer| layer.get("layer")?.as_str())
        .collect();

    (!layers.is_empty()).then(|| format!("OUT {}", layers.join(",")))
}

async fn read_reply(reader: &mut BufReader<TcpStream>) -> Result<String, String> {
    let mut line = String::new();

    match timeout(REPLY_TIMEOUT, reader.read_line(&mut line)).await {
        Ok(Ok(0)) => Err("connection closed".to_string()),
        Ok(Ok(_)) => Ok(line.trim().to_string()),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("no reply".to_string()),
    }
}

async fn write_command(reader: &mut BufReader<TcpStream>, command: &str) -> Result<String, String> {
    reader
        .get_mut()
        .write_all(format!("{command}\n").as_bytes())
        .await
        .map_err(|error| error.to_string())?;

    read_reply(reader).await
}

/// Sends commands over one connection and returns each command with its reply.
/// With `clear_first` all layers on air are taken out before the commands.
pub async fn send(
    address: &str,
    token: &str,
    commands: &[String],
    clear_first: bool,
) -> Result<Vec<(String, String)>, String> {
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .map_err(|_| format!("connect to {address}: timeout"))?
        .map_err(|error| format!("connect to {address}: {error}"))?;
    let mut reader = BufReader::new(stream);
    let mut replies = Vec::with_capacity(commands.len());

    if !token.is_empty() {
        let reply = write_command(&mut reader, &format!("AUTH {token}")).await?;

        if !reply.starts_with("OK") {
            return Err(format!("AUTH: {reply}"));
        }
    }

    if clear_first {
        let state = write_command(&mut reader, "STATE").await?;

        if let Some(out) = out_command(&state) {
            let reply = write_command(&mut reader, &out).await?;
            replies.push((out, reply));
        }
    }

    for command in commands {
        let reply = write_command(&mut reader, command).await?;
        replies.push((command.clone(), reply));
    }

    Ok(replies)
}

/// Renders and sends the secondary events for a clip that starts now.
pub async fn spawn_clip_events(manager: &ChannelManager, node: &Media) {
    let config = manager.config.read().await.vizrs.clone();

    if !config.enable {
        return;
    }

    let next = match node.index {
        Some(index) => manager.current_list.lock().await.get(index + 1).cloned(),
        None => None,
    };
    let ctx = ClipContext::new(node, next.as_ref());
    let commands = render_commands(&config, node, &ctx);
    let clear_first = node.ad;

    if commands.is_empty() && !clear_first {
        return;
    }

    let channel_id = manager.id;

    tokio::spawn(async move {
        let _order = SEND_ORDER.lock().await;

        match send(&config.address, &config.token, &commands, clear_first).await {
            Ok(replies) => {
                for (command, reply) in &replies {
                    if reply.starts_with("OK") {
                        debug!(channel = channel_id; "VizRS {command}: {reply}");
                    } else {
                        warn!(channel = channel_id; "VizRS {command}: {reply}");
                    }
                }
            }
            Err(error) => error!(channel = channel_id; "VizRS: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    fn media(title: Option<&str>, source: &str, ad: bool) -> Media {
        Media {
            title: title.map(str::to_string),
            source: source.to_string(),
            out: 90.0,
            ad,
            ..Media::default()
        }
    }

    fn config() -> Vizrs {
        Vizrs {
            enable: true,
            address: String::new(),
            token: String::new(),
            on_clip: "UPDATE 2001 teraz={title}|potem={next_title} o {next_start}\n\n# comment\nTAKE 1001"
                .to_string(),
            on_ad: "OUT main".to_string(),
            burn_url: String::new(),
        }
    }

    #[test]
    fn renders_placeholders_and_skips_comments() {
        let current = media(Some("Wiadomości | 19:30"), "/media/news.mp4", false);
        let mut next = media(None, "/media/Pogoda dla regionu.mp4", false);
        next.begin = Some(19.0 * 3600.0 + 45.0 * 60.0);

        let ctx = ClipContext::new(&current, Some(&next));
        let commands = render_commands(&config(), &current, &ctx);

        assert_eq!(
            commands,
            vec![
                "UPDATE 2001 teraz=Wiadomości / 19:30|potem=Pogoda dla regionu o 19:45",
                "TAKE 1001",
            ]
        );
    }

    #[test]
    fn ad_clips_use_only_ad_template() {
        let mut current = media(None, "/media/spot.mp4", true);
        current.vizrs = vec!["TAKE 1001".to_string()];

        let ctx = ClipContext::new(&current, None);
        let commands = render_commands(&config(), &current, &ctx);

        assert_eq!(commands, vec!["OUT main"]);
        assert_eq!(render_line("{duration}|{next_start}", &ctx), "00:01:30|");
    }

    #[test]
    fn out_command_covers_layers_on_air() {
        let reply = r#"OK {"layers":[{"layer":"main","page":"1001"},{"layer":"logo"}],"rev":3}"#;

        assert_eq!(out_command(reply), Some("OUT main,logo".to_string()));
        assert_eq!(out_command(r#"OK {"layers":[]}"#), None);
        assert_eq!(out_command("ERR busy"), None);
    }

    #[tokio::test]
    async fn sends_auth_and_collects_replies() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut received = Vec::new();

            for _ in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let reply = if line.starts_with("TAKE 9") {
                    "ERR no page\n"
                } else {
                    "OK {}\n"
                };
                reader.get_mut().write_all(reply.as_bytes()).await.unwrap();
                received.push(line.trim().to_string());
            }

            received
        });

        let commands = vec!["TAKE 1001".to_string(), "TAKE 9".to_string()];
        let replies = send(&address, "secret", &commands, false).await.unwrap();

        assert_eq!(
            replies,
            vec![
                ("TAKE 1001".to_string(), "OK {}".to_string()),
                ("TAKE 9".to_string(), "ERR no page".to_string()),
            ]
        );
        assert_eq!(
            server.await.unwrap(),
            vec!["AUTH secret", "TAKE 1001", "TAKE 9"]
        );
    }

    #[tokio::test]
    async fn unreachable_server_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        drop(listener);

        assert!(
            send(&address, "", &["PING".to_string()], false)
                .await
                .is_err()
        );
    }
}
