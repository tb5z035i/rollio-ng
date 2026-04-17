use std::error::Error;
use std::path::{Path, PathBuf};

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use rollio_types::config::UiRuntimeConfig;
use serde::Serialize;
use tokio_tungstenite::tungstenite::protocol::frame::{
    coding::CloseCode, CloseFrame as TungsteniteCloseFrame,
};
use tokio_tungstenite::tungstenite::protocol::Message as TungsteniteMessage;
use tower_http::services::{ServeDir, ServeFile};

const BROWSER_CONTROL_WEBSOCKET_PATH: &str = "/ws/control";
const BROWSER_PREVIEW_WEBSOCKET_PATH: &str = "/ws/preview";

#[derive(Parser, Debug)]
#[command(name = "rollio-ui-server")]
#[command(about = "Serve the Rollio browser UI and runtime config")]
struct Args {
    /// TOML file containing UiRuntimeConfig
    #[arg(long, value_name = "PATH", conflicts_with = "config_inline")]
    config: Option<PathBuf>,

    /// Inline TOML containing UiRuntimeConfig
    #[arg(long, value_name = "TOML", conflicts_with = "config")]
    config_inline: Option<String>,

    /// Path to the built frontend assets
    #[arg(long, value_name = "PATH", default_value = "ui/web/dist")]
    asset_dir: PathBuf,
}

#[derive(Clone)]
struct AppState {
    browser_runtime_config: BrowserRuntimeConfig,
    upstream_control_websocket_url: String,
    upstream_preview_websocket_url: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct BrowserRuntimeConfig {
    control_websocket_url: String,
    preview_websocket_url: String,
    episode_key_bindings: BrowserEpisodeKeyBindings,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct BrowserEpisodeKeyBindings {
    start_key: String,
    stop_key: String,
    keep_key: String,
    discard_key: String,
}

fn load_runtime_config(args: &Args) -> Result<UiRuntimeConfig, Box<dyn Error>> {
    let config = if let Some(config_path) = &args.config {
        std::fs::read_to_string(config_path)?.parse::<UiRuntimeConfig>()?
    } else if let Some(config_inline) = &args.config_inline {
        config_inline.parse::<UiRuntimeConfig>()?
    } else {
        UiRuntimeConfig::default()
    };

    Ok(config)
}

fn resolve_asset_dir(asset_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let resolved = if asset_dir.is_absolute() {
        asset_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(asset_dir)
    };

    if !resolved.exists() {
        return Err(format!(
            "web ui bundle not found at {}. Run `cd ui/web && npm run build` first.",
            resolved.display()
        )
        .into());
    }

    Ok(resolved)
}

fn browser_runtime_config(config: &UiRuntimeConfig) -> BrowserRuntimeConfig {
    BrowserRuntimeConfig {
        control_websocket_url: BROWSER_CONTROL_WEBSOCKET_PATH.to_string(),
        preview_websocket_url: BROWSER_PREVIEW_WEBSOCKET_PATH.to_string(),
        episode_key_bindings: BrowserEpisodeKeyBindings {
            start_key: config.start_key.clone(),
            stop_key: config.stop_key.clone(),
            keep_key: config.keep_key.clone(),
            discard_key: config.discard_key.clone(),
        },
    }
}

fn build_app_state(config: &UiRuntimeConfig) -> Result<AppState, Box<dyn Error>> {
    let upstream_control_websocket_url = config
        .control_websocket_url
        .clone()
        .ok_or("ui runtime config did not produce an upstream control websocket url")?;
    let upstream_preview_websocket_url = config
        .preview_websocket_url
        .clone()
        .ok_or("ui runtime config did not produce an upstream preview websocket url")?;

    Ok(AppState {
        browser_runtime_config: browser_runtime_config(config),
        upstream_control_websocket_url,
        upstream_preview_websocket_url,
    })
}

fn build_app(state: AppState, asset_dir: PathBuf, index_file: PathBuf) -> Router {
    Router::new()
        .route("/api/runtime-config", get(runtime_config_handler))
        .route(BROWSER_CONTROL_WEBSOCKET_PATH, get(control_websocket_handler))
        .route(BROWSER_PREVIEW_WEBSOCKET_PATH, get(preview_websocket_handler))
        .fallback_service(ServeDir::new(asset_dir).not_found_service(ServeFile::new(index_file)))
        .with_state(state)
}

fn display_host(host: &str) -> &str {
    match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        _ => host,
    }
}

async fn runtime_config_handler(State(state): State<AppState>) -> Json<BrowserRuntimeConfig> {
    Json(state.browser_runtime_config)
}

async fn control_websocket_handler(
    websocket: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    let upstream = state.upstream_control_websocket_url.clone();
    websocket.on_upgrade(move |socket| proxy_websocket(socket, upstream))
}

async fn preview_websocket_handler(
    websocket: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    let upstream = state.upstream_preview_websocket_url.clone();
    websocket.on_upgrade(move |socket| proxy_websocket(socket, upstream))
}

async fn proxy_websocket(mut downstream: WebSocket, upstream_websocket_url: String) {
    let (upstream, _) =
        match tokio_tungstenite::connect_async(upstream_websocket_url.as_str()).await {
            Ok(connection) => connection,
            Err(error) => {
                // Don't escalate this to stderr — the preview upstream is
                // intentionally absent during the setup wizard outside of
                // identify, and the browser client is expected to retry on
                // close.
                log_proxy_connect_failure(&upstream_websocket_url, &error);
                let _ = downstream.send(Message::Close(None)).await;
                return;
            }
        };

    let (mut downstream_sink, mut downstream_stream) = downstream.split();
    let (mut upstream_sink, mut upstream_stream) = upstream.split();

    let downstream_to_upstream = async {
        while let Some(message_result) = downstream_stream.next().await {
            let message = match message_result {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("rollio: websocket proxy read error from browser client: {error}");
                    break;
                }
            };
            let should_close = matches!(message, Message::Close(_));
            if let Some(upstream_message) = downstream_message_to_upstream(message) {
                if let Err(error) = upstream_sink.send(upstream_message).await {
                    eprintln!("rollio: websocket proxy write error to upstream: {error}");
                    break;
                }
            }
            if should_close {
                break;
            }
        }
        let _ = upstream_sink.close().await;
    };

    let upstream_to_downstream = async {
        while let Some(message_result) = upstream_stream.next().await {
            let message = match message_result {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("rollio: websocket proxy read error from upstream: {error}");
                    break;
                }
            };
            let should_close = matches!(message, TungsteniteMessage::Close(_));
            if let Some(downstream_message) = upstream_message_to_downstream(message) {
                if let Err(error) = downstream_sink.send(downstream_message).await {
                    eprintln!("rollio: websocket proxy write error to browser client: {error}");
                    break;
                }
            }
            if should_close {
                break;
            }
        }
        let _ = downstream_sink.close().await;
    };

    tokio::select! {
        _ = downstream_to_upstream => {}
        _ = upstream_to_downstream => {}
    }
}

fn log_proxy_connect_failure(upstream_url: &str, error: &tokio_tungstenite::tungstenite::Error) {
    // Quiet logging when the upstream is simply not yet listening (which is
    // the normal state for the preview channel during setup wizard).
    eprintln!("rollio: websocket proxy upstream {upstream_url} unavailable: {error}");
}

fn downstream_message_to_upstream(message: Message) -> Option<TungsteniteMessage> {
    match message {
        Message::Text(text) => Some(TungsteniteMessage::Text(text.to_string().into())),
        Message::Binary(bytes) => Some(TungsteniteMessage::Binary(bytes)),
        Message::Ping(bytes) => Some(TungsteniteMessage::Ping(bytes)),
        Message::Pong(bytes) => Some(TungsteniteMessage::Pong(bytes)),
        Message::Close(frame) => Some(TungsteniteMessage::Close(frame.map(|frame| {
            TungsteniteCloseFrame {
                code: CloseCode::from(frame.code),
                reason: frame.reason.to_string().into(),
            }
        }))),
    }
}

fn upstream_message_to_downstream(message: TungsteniteMessage) -> Option<Message> {
    match message {
        TungsteniteMessage::Text(text) => Some(Message::Text(text.to_string().into())),
        TungsteniteMessage::Binary(bytes) => Some(Message::Binary(bytes)),
        TungsteniteMessage::Ping(bytes) => Some(Message::Ping(bytes)),
        TungsteniteMessage::Pong(bytes) => Some(Message::Pong(bytes)),
        TungsteniteMessage::Close(frame) => Some(Message::Close(frame.map(|frame| CloseFrame {
            code: frame.code.into(),
            reason: frame.reason.to_string().into(),
        }))),
        TungsteniteMessage::Frame(_) => None,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let runtime_config = load_runtime_config(&args)?;
    let asset_dir = resolve_asset_dir(&args.asset_dir)?;
    let index_file = asset_dir.join("index.html");
    if !index_file.exists() {
        return Err(format!(
            "web ui entrypoint not found at {}. Run `cd ui/web && npm run build` first.",
            index_file.display()
        )
        .into());
    }

    let state = build_app_state(&runtime_config)?;
    let app = build_app(state, asset_dir, index_file);

    let listener = tokio::net::TcpListener::bind((
        runtime_config.http_host.as_str(),
        runtime_config.http_port,
    ))
    .await?;
    eprintln!(
        "rollio: web ui available at http://{}:{}",
        display_host(&runtime_config.http_host),
        runtime_config.http_port
    );
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::fs;
    use std::net::SocketAddr;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::protocol::Message as TungsteniteMessage;

    fn empty_args() -> Args {
        Args {
            config: None,
            config_inline: None,
            asset_dir: PathBuf::from("ui/web/dist"),
        }
    }

    fn sample_runtime_config() -> UiRuntimeConfig {
        r#"
control_websocket_url = "ws://127.0.0.1:9091"
preview_websocket_url = "ws://127.0.0.1:19090"
start_key = "s"
stop_key = "e"
keep_key = "k"
discard_key = "x"
http_host = "127.0.0.1"
http_port = 3000
"#
        .parse::<UiRuntimeConfig>()
        .expect("inline config should parse")
    }

    fn temp_asset_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let asset_dir = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
        fs::create_dir_all(&asset_dir).expect("temp asset dir should exist");
        fs::write(
            asset_dir.join("index.html"),
            "<!doctype html>\n<title>Rollio UI</title>\n",
        )
        .expect("index.html should be written");
        asset_dir
    }

    async fn spawn_app_server(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("test listener should bind");
        let address = listener.local_addr().expect("listener should expose addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test app should keep serving");
        });
        (address, handle)
    }

    async fn spawn_echo_upstream(label: &'static str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("upstream listener should bind");
        let addr = listener.local_addr().expect("upstream addr");
        let handle = tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                if let Ok(mut ws) = accept_async(stream).await {
                    let _ = ws
                        .send(TungsteniteMessage::Text(format!("hello-from-{label}").into()))
                        .await;
                    while let Some(Ok(msg)) = ws.next().await {
                        if matches!(msg, TungsteniteMessage::Close(_)) {
                            break;
                        }
                        if let TungsteniteMessage::Text(t) = msg {
                            let _ = ws
                                .send(TungsteniteMessage::Text(
                                    format!("{label}-echo:{t}").into(),
                                ))
                                .await;
                        }
                    }
                }
            }
        });
        (addr, handle)
    }

    #[test]
    fn default_runtime_config_loads() {
        let config = load_runtime_config(&empty_args()).expect("default config should load");
        assert_eq!(config.http_host, "127.0.0.1");
        assert_eq!(config.http_port, 3000);
    }

    #[test]
    fn browser_runtime_config_uses_same_origin_websocket_paths() {
        let state = build_app_state(&sample_runtime_config()).expect("app state should be built");

        assert_eq!(
            state.browser_runtime_config.control_websocket_url,
            BROWSER_CONTROL_WEBSOCKET_PATH
        );
        assert_eq!(
            state.browser_runtime_config.preview_websocket_url,
            BROWSER_PREVIEW_WEBSOCKET_PATH
        );
        assert_eq!(
            state.browser_runtime_config.episode_key_bindings.start_key,
            "s"
        );
        assert_eq!(state.upstream_control_websocket_url, "ws://127.0.0.1:9091");
        assert_eq!(state.upstream_preview_websocket_url, "ws://127.0.0.1:19090");
    }

    #[tokio::test]
    async fn each_path_proxies_to_its_own_upstream() {
        let (control_addr, control_task) = spawn_echo_upstream("control").await;
        let (preview_addr, preview_task) = spawn_echo_upstream("preview").await;

        let mut runtime_config = sample_runtime_config();
        runtime_config.control_websocket_url = Some(format!("ws://{control_addr}"));
        runtime_config.preview_websocket_url = Some(format!("ws://{preview_addr}"));

        let asset_dir = temp_asset_dir("rollio-ui-server-tests-paths");
        let app = build_app(
            build_app_state(&runtime_config).expect("app state should be built"),
            asset_dir.clone(),
            asset_dir.join("index.html"),
        );
        let (app_addr, app_task) = spawn_app_server(app).await;

        let control_url = format!("ws://{app_addr}{BROWSER_CONTROL_WEBSOCKET_PATH}");
        let (mut control_ws, _) = tokio_tungstenite::connect_async(control_url)
            .await
            .expect("control proxy should connect");
        let control_hello = control_ws
            .next()
            .await
            .expect("control hello frame")
            .expect("frame ok");
        assert_eq!(
            control_hello,
            TungsteniteMessage::Text("hello-from-control".into())
        );

        let preview_url = format!("ws://{app_addr}{BROWSER_PREVIEW_WEBSOCKET_PATH}");
        let (mut preview_ws, _) = tokio_tungstenite::connect_async(preview_url)
            .await
            .expect("preview proxy should connect");
        let preview_hello = preview_ws
            .next()
            .await
            .expect("preview hello frame")
            .expect("frame ok");
        assert_eq!(
            preview_hello,
            TungsteniteMessage::Text("hello-from-preview".into())
        );

        control_ws
            .close(None)
            .await
            .expect("control websocket should close cleanly");
        preview_ws
            .close(None)
            .await
            .expect("preview websocket should close cleanly");
        control_task.await.expect("control upstream finishes");
        preview_task.await.expect("preview upstream finishes");
        app_task.abort();
        let _ = app_task.await;
        let _ = fs::remove_dir_all(asset_dir);
    }
}
