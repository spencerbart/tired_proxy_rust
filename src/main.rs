use std::{net::SocketAddr, time::Duration};

use axum::{
    error_handling::HandleErrorLayer,
    extract::State,
    http::{uri::Uri, Request, Response},
    routing::get,
    BoxError, Router,
};
use clap::Parser;
use hyper::{client::HttpConnector, Body, StatusCode};
use tower::{timeout::TimeoutLayer, ServiceBuilder};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{
    fmt, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};
use validator::Validate;

type Client = hyper::client::Client<HttpConnector, Body>;

#[derive(Parser, Validate)]
struct AppArgs {
    #[arg(short, long, default_value = "http://127.0.0.1:3000")]
    #[validate(url)]
    forward: String,
    #[arg(short, long, default_value_t = 8080)]
    port: u16,
}

static GLOBAL_TIMER: state::Storage<tokio::time::Instant> = state::Storage::new();

#[tokio::main]
async fn main() {
    let args = AppArgs::parse();

    GLOBAL_TIMER.set(tokio::time::Instant::now());
    tokio::spawn(async {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            timer().await;
        }
    });

    let client = Client::new();

    let filter = EnvFilter::builder()
        .with_default_directive("tower_http=trace".parse().unwrap())
        .from_env_lossy();

    // initializes tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    let app = Router::new()
        .route("/", get(handler).post(handler))
        .route("/*route", get(handler).post(handler))
        .with_state(client)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(HandleErrorLayer::new(|_: BoxError| async {
                    StatusCode::REQUEST_TIMEOUT
                }))
                .layer(TimeoutLayer::new(Duration::from_secs(10))),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    println!("reverse proxy listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn handler(State(client): State<Client>, mut req: Request<Body>) -> Response<Body> {
    GLOBAL_TIMER.set(tokio::time::Instant::now());

    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(path);

    let uri = format!("http://0.0.0.0:3000{}", path_query);

    *req.uri_mut() = Uri::try_from(uri).unwrap();

    client.request(req).await.unwrap()
}

async fn timer() {
    if GLOBAL_TIMER.get().elapsed().as_secs() > 900 {
        println!("server timed out");
        std::process::exit(0);
    }
}
