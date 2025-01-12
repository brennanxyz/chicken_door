use crate::{config::Config, door::DoorStatus};
use axum::{
    debug_handler,
    http::{header::HeaderMap, StatusCode},
    Extension, Json,
};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Write},
};
use tracing::{event, Level};

#[debug_handler]
pub async fn get_door_status(
    Extension(config): Extension<Config>,
    headers: HeaderMap,
) -> Result<Json<DoorStatus>, StatusCode> {
    event!(Level::INFO, "GET | /get_door_status");

    let access_pass = match headers.get("x-access-key") {
        Some(access_pass) => access_pass,
        None => {
            event!(Level::WARN, "No access key provided");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if *config.access_key == *access_pass {
        // get status from file
        let status_file = File::open(config.status_file).expect("Missing status file");
        let status_reader = BufReader::new(status_file);
        let door_status: DoorStatus =
            serde_json::from_reader(status_reader).expect("Bad door status structure");
        Ok(Json(door_status))
    } else {
        event!(Level::WARN, "Unauthorized access attempt");
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[debug_handler]
pub async fn update_door_status(
    Extension(config): Extension<Config>,
    headers: HeaderMap,
    Json(door_status): Json<DoorStatus>,
) -> Result<Json<DoorStatus>, StatusCode> {
    event!(Level::INFO, "PUT | /update_door_status");

    let access_pass = match headers.get("x-access-key") {
        Some(access_pass) => access_pass,
        None => {
            event!(Level::WARN, "No access key provided");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if *config.access_key == *access_pass {
        // write status to file
        let mut status_file = OpenOptions::new()
            .write(true)
            .create(false)
            .append(false)
            .open(config.status_file)
            .expect("Missing status file");

        let status_string =
            serde_json::to_string(&door_status).expect("Couldn't stringify payload");

        status_file
            .write_all(status_string.as_bytes())
            .expect("File write error");

        Ok(Json(door_status))
    } else {
        event!(Level::WARN, "Unauthorized access attempt");
        Err(StatusCode::UNAUTHORIZED)
    }
}
