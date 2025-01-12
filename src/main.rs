mod config;
mod door;
mod routes;
mod sun;

use config::Config;
use routes::{get_door_status, update_door_status};
use sun::update_status_file;

use core::panic;
use std::{fs::File, thread, time};

use axum::{
    http::Method,
    routing::{get, put},
    Extension, Router,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{event, Level};
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[tokio::main]
async fn main() {
    // run loop
    //   check current time
    //   check current state and set state if needed
    //   see if change should be made
    //   write file with suggested change

    // establish logging
    let logfile = tracing_appender::rolling::hourly("./logs", "chicken.log");

    let stdout = std::io::stdout.with_max_level(Level::INFO);
    tracing_subscriber::fmt()
        .pretty()
        .with_writer(stdout.and(logfile))
        .init();

    event!(Level::INFO, "Hello, chickens! Rise and shine!");

    // get config
    let config = Config::initialize();
    let config_clone = config.clone();
    let config_clone_two = config.clone();

    // check that appropriate files exist
    match File::open(config.schedule_file) {
        Ok(_) => event!(Level::INFO, "Found schedule"),
        Err(e) => {
            event!(Level::ERROR, "Schedule not found | {}", e);
            panic!("Schedule not found. Terminating server");
        }
    }

    match File::open(config.status_file) {
        Ok(_) => event!(Level::INFO, "Found status file"),
        Err(e) => {
            event!(Level::ERROR, "Status file not found | {}", e);
            panic!("Status file not found. Terminating server");
        }
    }

    // spawn supervisory loop
    thread::spawn(move || loop {
        let interval_seconds = time::Duration::from_secs(config.interval_seconds);
        thread::sleep(interval_seconds);
        update_status_file(&config_clone);
    });

    // establish routes
    let router = Router::new()
        .route(
            "/",
            get(|| async { "Oh, Chicken, Chicken, you can't roost too high for me" }),
        )
        .route("/get_door_status", get(get_door_status))
        .route("/update_door_status", put(update_door_status))
        .layer(Extension(config_clone_two))
        .layer(
            CorsLayer::new()
                .allow_methods([Method::GET, Method::PUT])
                .allow_origin(Any),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Bad listener.");

    event!(Level::INFO, "Server listening on port 3000");

    axum::serve(listener, router.into_make_service())
        .await
        .expect("Bad server.");
}
