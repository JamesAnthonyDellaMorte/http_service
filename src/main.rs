use futures::{FutureExt, SinkExt, StreamExt};
use warp::http::StatusCode;
use tokio::{
    process::Command,
    sync::broadcast,
    time::{interval, Duration},
};
use warp::{
    http::Uri,
    ws::{Message, WebSocket, Ws},
    Filter,
};


async fn service_status_sender(tx: broadcast::Sender<String>) {
    let mut interval = interval(Duration::from_secs(1)); // Check every 5 seconds

    loop {
        interval.tick().await;
        let output = Command::new("systemctl")
            .arg("status")
            .arg("tvlimit.service") // Replace with your systemd service name
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
        .arg("tvlimit.service") // Replace with your systemd service name
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

    // Route to serve static/index.html for the root path

    // Inside your main function, before starting the server, add a route for static files:
    let static_route = warp::path("static").and(warp::fs::dir("./static"));
    let redirect_route =
        warp::path::end().map(|| warp::redirect(Uri::from_static("/static/index.html")));
    // Include this route in your warp::serve call alongside the existing WebSocket route
    let restart_route = warp::path("restart")
    .and(warp::post())
    .and_then(restart_service);
    let routes = status_route.or(static_route).or(redirect_route).or(restart_route);
    
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}