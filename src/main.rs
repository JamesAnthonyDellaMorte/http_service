use futures::{FutureExt, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::OpenOptions,
    io::AsyncWriteExt,
    process::Command,
    sync::broadcast,
    time::{interval, Duration},
};
use warp::http::StatusCode;
use warp::{
    http::Uri,
    ws::{Message, WebSocket, Ws},
    Filter,
};

#[derive(Deserialize, Serialize)]
struct UpdateContent {
    content: String,
}

async fn update_and_restart_handler(
    body: UpdateContent,
) -> Result<impl warp::Reply, warp::Rejection> {
    match update_tvlimit_and_restart_service(&body.content).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => {
            eprintln!("Failed to update and restart: {:?}", e);
            Ok(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn update_tvlimit_and_restart_service(
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Stop the tvlimit.service
    Command::new("systemctl")
        .arg("stop")
        .arg("tvlimit.service")
        .output()
        .await?;

    let file_path = "/root/tvlimit/tvtimer.txt";
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(file_path)
        .await?;

    file.write_all(contents.as_bytes()).await?;
    file.sync_all().await?;

    // Start the tvlimit.service
    Command::new("systemctl")
        .arg("start")
        .arg("tvlimit.service")
        .output()
        .await?;

    Ok(())
}

async fn service_status_sender(tx: broadcast::Sender<String>) {
    let mut interval = interval(Duration::from_secs(1)); // Check every second

    loop {
        interval.tick().await;
        let output = Command::new("systemctl")
            .arg("status")
            .arg("tvlimit.service")
            .output()
            .await
            .expect("Failed to execute systemctl");

        let status = String::from_utf8_lossy(&output.stdout).into_owned();
        tx.send(status).unwrap_or_default();
    }
}

async fn restart_service() -> Result<impl warp::Reply, warp::Rejection> {
    let output = Command::new("systemctl")
        .arg("restart")
        .arg("tvlimit.service")
        .output()
        .await;

    match output {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn stop_service() -> Result<impl warp::Reply, warp::Rejection> {
    let output = Command::new("systemctl")
        .arg("stop")
        .arg("tvlimit.service")
        .output()
        .await;

    match output {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn start_service() -> Result<impl warp::Reply, warp::Rejection> {
    let output = Command::new("systemctl")
        .arg("start")
        .arg("tvlimit.service")
        .output()
        .await;

    match output {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn user_connected(ws: WebSocket, mut rx: broadcast::Receiver<String>) {
    let (mut ws_tx, _) = ws.split();
    while let Ok(status) = rx.recv().await {
        let message = Message::text(status);
        ws_tx.send(message).await.expect("Failed to send message");
    }
}

#[tokio::main]
async fn main() {
    let (tx, _) = broadcast::channel(10);
    tokio::spawn(service_status_sender(tx.clone()));

    let status_route = warp::path("status").and(warp::ws()).map(move |ws: Ws| {
        let tx = tx.clone();
        ws.on_upgrade(move |socket| {
            let rx = tx.subscribe();
            user_connected(socket, rx).map(|_| ())
        })
    });

    let update_and_restart_route = warp::path("update-and-restart")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(update_and_restart_handler);

    let static_route = warp::path("static").and(warp::fs::dir("./static"));
    let redirect_route =
        warp::path::end().map(|| warp::redirect(Uri::from_static("/static/index.html")));

    let restart_route = warp::path("restart")
        .and(warp::post())
        .and_then(restart_service);

    let stop_route = warp::path("stop").and(warp::post()).and_then(stop_service);

    let start_route = warp::path("start")
        .and(warp::post())
        .and_then(start_service);

    let routes = status_route
        .or(static_route)
        .or(update_and_restart_route)
        .or(redirect_route)
        .or(restart_route)
        .or(stop_route)
        .or(start_route);

    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}
