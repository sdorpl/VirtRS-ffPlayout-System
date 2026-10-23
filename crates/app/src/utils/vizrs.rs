//! Secondary events for a VizRS graphics server.
//!
//! When a clip starts, the channel's command template (`on_clip`, or `on_ad`
//! for commercials) and the clip's own `vizrs` list are rendered and sent to
//! VizRS over its line-based TCP protocol (`TAKE`, `UPDATE`, `OUT`, ...).
//! Every command gets one `OK ...` or `ERR ...` reply line.
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
/// Empty lines and lines starting with `#` are ignored.
pub fn render_commands(config: &Vizrs, media: &Media, ctx: &ClipContext) -> Vec<String> {
    let template = if media.ad {
        &config.on_ad
    } else {
        &config.on_clip
    };

    template
        .lines()
        .chain(media.vizrs.iter().map(String::as_str))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| render_line(line, ctx))
        .collect()
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

/// Sends commands over one connection and returns the reply for each one.
pub async fn send(address: &str, token: &str, commands: &[String]) -> Result<Vec<String>, String> {
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .map_err(|_| format!("connect to {address}: timeout"))?
        .map_err(|error| format!("connect to {address}: {error}"))?;
    let mut reader = BufReader::new(stream);
    let mut replies = Vec::with_capacity(commands.len());

    if !token.is_empty() {
        reader
            .get_mut()
            .write_all(format!("AUTH {token}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        let reply = read_reply(&mut reader).await?;

        if !reply.starts_with("OK") {
            return Err(format!("AUTH: {reply}"));
        }
    }

    for command in commands {
        reader
            .get_mut()
            .write_all(format!("{command}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        replies.push(read_reply(&mut reader).await?);
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

    if commands.is_empty() {
        return;
    }

    let channel_id = manager.id;

    tokio::spawn(async move {
        let _order = SEND_ORDER.lock().await;

        match send(&config.address, &config.token, &commands).await {
            Ok(replies) => {
                for (command, reply) in commands.iter().zip(&replies) {
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
    fn ad_clips_use_ad_template_and_clip_commands_follow() {
        let mut current = media(None, "/media/spot.mp4", true);
        current.vizrs = vec!["CONTINUE logo".to_string(), " ".to_string()];

        let ctx = ClipContext::new(&current, None);
        let commands = render_commands(&config(), &current, &ctx);

        assert_eq!(commands, vec!["OUT main", "CONTINUE logo"]);
        assert_eq!(render_line("{duration}|{next_start}", &ctx), "00:01:30|");
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
        let replies = send(&address, "secret", &commands).await.unwrap();

        assert_eq!(replies, vec!["OK {}", "ERR no page"]);
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

        assert!(send(&address, "", &["PING".to_string()]).await.is_err());
    }
}
