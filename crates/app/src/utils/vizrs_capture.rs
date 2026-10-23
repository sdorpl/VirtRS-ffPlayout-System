//! Burn-in of VizRS graphics: a headless Chromium renders the VizRS `/output`
//! page with a transparent background, and every changed frame (DevTools
//! `Page.startScreencast`, PNG with alpha) replaces the channel's
//! [`GraphicsLayer`], which the engine blends over the program video.
//!
//! Chromium only sends frames when the page changes, so static graphics cost
//! almost nothing. The browser runs without GPU (`--disable-gpu`), as on the
//! target servers. A crashed or unreachable browser is restarted; until then
//! the layer is cleared and playout continues without graphics.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ff_engine::GraphicsLayer;
use futures_util::{SinkExt, StreamExt};
use log::*;
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::time::{Duration, Instant, sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::{CancellationToken, DropGuard};

const START_TIMEOUT: Duration = Duration::from_secs(15);
const RESTART_DELAY: Duration = Duration::from_secs(3);
const BROWSER_NAMES: [&str; 6] = [
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "chrome",
    "headless_shell",
];

/// Parameters of one burn-in capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub channel_id: i32,
}

/// Starts the capture; it stops and clears the layer when the guard drops.
pub fn start(config: CaptureConfig, layer: GraphicsLayer) -> DropGuard {
    let cancel = CancellationToken::new();
    let token = cancel.clone();

    tokio::spawn(async move {
        supervise(config, layer, token).await;
    });

    cancel.drop_guard()
}

/// `VIZRS_BROWSER`, or the first Chromium-like browser found in `PATH`.
fn find_browser() -> Option<PathBuf> {
    if let Some(path) = env::var_os("VIZRS_BROWSER").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }

    let paths = env::var_os("PATH")?;

    env::split_paths(&paths)
        .flat_map(|dir| BROWSER_NAMES.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

async fn supervise(config: CaptureConfig, layer: GraphicsLayer, cancel: CancellationToken) {
    let id = config.channel_id;

    let Some(browser) = find_browser() else {
        error!(channel = id;
            "VizRS burn-in: no Chromium found (install chromium or set VIZRS_BROWSER)"
        );
        return;
    };

    loop {
        let result = tokio::select! {
            result = capture(&browser, &config, &layer) => result,
            _ = cancel.cancelled() => Ok(()),
        };

        layer.clear();

        if cancel.is_cancelled() {
            break;
        }

        if let Err(error) = result {
            error!(channel = id; "VizRS burn-in: {error}; restarting browser");
        }

        tokio::select! {
            _ = sleep(RESTART_DELAY) => {}
            _ = cancel.cancelled() => break,
        }
    }
}

fn browser_command(browser: &Path, profile: &Path, config: &CaptureConfig) -> Command {
    let mut command = Command::new(browser);

    command
        .args([
            "--headless=new",
            "--disable-gpu",
            "--remote-debugging-port=0",
            "--hide-scrollbars",
            "--mute-audio",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--disable-backgrounding-occluded-windows",
            "--force-device-scale-factor=1",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--window-size={},{}", config.width, config.height))
        // The page opens in the startup tab: a tab created later over DevTools
        // stays hidden in headless mode and never paints.
        .arg(&config.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    // Containers often run as root, where Chromium refuses its sandbox.
    if env::var_os("VIZRS_BROWSER_NO_SANDBOX").is_some_and(|v| v == "1") {
        command.arg("--no-sandbox");
    }

    command
}

/// Waits for Chromium to write `DevToolsActivePort` (port, browser path).
async fn devtools_url(profile: &Path, child: &mut Child) -> Result<String, String> {
    let deadline = Instant::now() + START_TIMEOUT;
    let file = profile.join("DevToolsActivePort");

    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("browser exited at start ({status})"));
        }

        if let Ok(content) = tokio::fs::read_to_string(&file).await {
            let mut lines = content.lines();

            if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                return Ok(format!("ws://127.0.0.1:{}{}", port.trim(), path.trim()));
            }
        }

        sleep(Duration::from_millis(100)).await;
    }

    Err("browser did not open the DevTools port".to_string())
}

/// Minimal DevTools client over the browser websocket (flat sessions).
struct DevTools<S> {
    socket: S,
    next_id: u64,
}

impl<S> DevTools<S>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin,
{
    async fn send(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<u64, String> {
        self.next_id += 1;
        let mut message = json!({ "id": self.next_id, "method": method, "params": params });

        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }

        self.socket
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(|error| format!("{method}: {error}"))?;

        Ok(self.next_id)
    }

    async fn next_message(&mut self) -> Result<Value, String> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text).map_err(|error| error.to_string());
                }
                Some(Ok(Message::Close(_))) | None => {
                    return Err("DevTools connection closed".to_string());
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.to_string()),
            }
        }
    }

    /// Sends a command and waits for its result, skipping unrelated events.
    async fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let id = self.send(session, method, params).await?;

        let reply = timeout(START_TIMEOUT, async {
            loop {
                let message = self.next_message().await?;

                if message["id"].as_u64() == Some(id) {
                    return Ok::<Value, String>(message);
                }
            }
        })
        .await
        .map_err(|_| format!("{method}: no reply"))??;

        if let Some(error) = reply.get("error") {
            return Err(format!("{method}: {error}"));
        }

        Ok(reply["result"].clone())
    }
}

async fn capture(
    browser: &Path,
    config: &CaptureConfig,
    layer: &GraphicsLayer,
) -> Result<(), String> {
    let profile = env::temp_dir().join(format!(
        "vizrs-burnin-{}-{}",
        std::process::id(),
        config.channel_id
    ));
    let _ = tokio::fs::remove_dir_all(&profile).await;
    tokio::fs::create_dir_all(&profile)
        .await
        .map_err(|error| format!("profile {}: {error}", profile.display()))?;

    let mut child = browser_command(browser, &profile, config)
        .spawn()
        .map_err(|error| format!("start {}: {error}", browser.display()))?;
    let result = run_session(&mut child, &profile, config, layer).await;

    let _ = child.kill().await;
    let _ = tokio::fs::remove_dir_all(&profile).await;

    result
}

/// The startup tab once it shows the page (it starts as `about:blank`).
async fn page_target<S>(devtools: &mut DevTools<S>) -> Result<String, String>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin,
{
    let deadline = Instant::now() + START_TIMEOUT;

    while Instant::now() < deadline {
        let targets = devtools.call(None, "Target.getTargets", json!({})).await?;
        let page = targets["targetInfos"].as_array().and_then(|infos| {
            infos.iter().find(|info| {
                info["type"] == "page"
                    && info["url"]
                        .as_str()
                        .is_some_and(|url| url.starts_with("http"))
            })
        });

        if let Some(id) = page.and_then(|info| info["targetId"].as_str()) {
            return Ok(id.to_string());
        }

        sleep(Duration::from_millis(100)).await;
    }

    Err("page did not open".to_string())
}

async fn run_session(
    child: &mut Child,
    profile: &Path,
    config: &CaptureConfig,
    layer: &GraphicsLayer,
) -> Result<(), String> {
    let url = devtools_url(profile, child).await?;
    let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .map_err(|error| format!("DevTools connect: {error}"))?;
    let mut devtools = DevTools { socket, next_id: 0 };

    let target_id = page_target(&mut devtools).await?;
    let attached = devtools
        .call(
            None,
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
        )
        .await?;
    let session = attached["sessionId"]
        .as_str()
        .ok_or("no sessionId")?
        .to_string();
    let session = Some(session.as_str());

    devtools
        .call(
            session,
            "Emulation.setDeviceMetricsOverride",
            json!({ "width": config.width, "height": config.height, "deviceScaleFactor": 1, "mobile": false }),
        )
        .await?;
    devtools
        .call(
            session,
            "Emulation.setDefaultBackgroundColorOverride",
            json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }),
        )
        .await?;
    // Transparency only through DevTools: `--default-background-color=0`
    // crashes the renderer of current Chromium builds.
    devtools.call(session, "Page.enable", json!({})).await?;
    devtools
        .call(
            session,
            "Page.startScreencast",
            json!({ "format": "png", "maxWidth": config.width, "maxHeight": config.height, "everyNthFrame": 1 }),
        )
        .await?;

    info!(channel = config.channel_id; "VizRS burn-in: capturing {}", config.url);

    loop {
        let message = devtools.next_message().await?;

        match message["method"].as_str() {
            Some("Page.screencastFrame") => {
                let params = &message["params"];
                let ack = params["sessionId"].clone();
                let png = BASE64
                    .decode(params["data"].as_str().unwrap_or_default())
                    .map_err(|error| format!("frame data: {error}"))?;
                let frame_layer = layer.clone();
                let (width, height) = (config.width, config.height);

                // Decoding before the ack gives Chromium backpressure: it
                // sends the next frame only once this one is on the layer.
                let decoded =
                    tokio::task::spawn_blocking(move || frame_layer.set_png(&png, width, height))
                        .await
                        .map_err(|error| error.to_string())?;

                if let Err(error) = decoded {
                    warn!(channel = config.channel_id; "VizRS burn-in: bad frame: {error}");
                }

                devtools
                    .send(
                        session,
                        "Page.screencastFrameAck",
                        json!({ "sessionId": ack }),
                    )
                    .await?;
            }
            Some("Inspector.targetCrashed" | "Target.targetCrashed" | "Inspector.detached") => {
                return Err("page crashed".to_string());
            }
            Some("Target.detachedFromTarget") => return Err("page closed".to_string()),
            _ => {}
        }
    }
}
