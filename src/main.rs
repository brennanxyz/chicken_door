use std::fs::File;
use std::io::Read;
use std::thread;
use std::time;

use chrono::{Datelike, Duration, Local, Timelike, Utc};
use fern;
use log::{info, warn, error};
use reqwest;
use reqwest::RequestBuilder;
use serde::{Serialize, Deserialize};
use serde_json;
use tokio;
use toml;

#[derive(Deserialize)]
struct SunHappening {
    sunrise: f32,
    sunset: f32,
}

#[derive(Deserialize, Debug)]
enum DoorStatus {
    Open,
    Closed,
    Opening,
    Closing,
    Unknown,
}

#[derive(Deserialize, Debug)]
enum DoorAction {
    Open,
    Close,
    Pass,
}

#[derive(Deserialize, Debug)]
struct DoorResponse {
    record: DoorRecord,
}

#[derive(Serialize, Deserialize, Debug)]
struct DoorRecord {
    executed: bool,
    direction: String,
    amount: u8
}

#[derive(Deserialize)]
struct Config {
    interval_seconds: u64,
    hour_offset: i64,
    json_bin: JSONBin,
}

#[derive(Deserialize, Clone)]
struct JSONBin {
    base_url: String,
    master_key: String,
    access_key: String,
    bin_id: String,
}

#[tokio::main]
async fn main() {

    let (config, sun_happenings, client_getter, client_putter) = initialize();

    info!("entering main loop");

    loop {
        let (now_seconds, ordinal) = get_now(config.hour_offset);
        let door_status = get_door_status(client_getter.try_clone()).await;
        let is_daylight = is_daylight(now_seconds, sun_happenings[ordinal].sunrise, sun_happenings[ordinal].sunset);
        let suggestion = suggest_action(door_status, is_daylight);
        politely_carry_out_suggestion(suggestion, client_putter.try_clone()).await;

        let interval_seconds = time::Duration::from_secs(config.interval_seconds);
        thread::sleep(interval_seconds);
    }; 
}

fn initialize() -> (Config, Vec<SunHappening>, RequestBuilder, RequestBuilder) {
    // set up logging
    let mut log_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{},{},{}",
                Local::now().format("%Y-%m-%d|%H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info);

    log_config = log_config.chain(fern::log_file("chicken_door.log").unwrap());
    log_config.apply().unwrap();

    // get config
    let mut file = File::open(".config.toml").unwrap();
    let mut buff = String::new();
    file.read_to_string(&mut buff).unwrap();
    let config: Config = toml::from_str(&buff).unwrap();

    // get sunrise/set times
    let mut file = File::open("files/schedule.json").unwrap();
    let mut buff = String::new();
    file.read_to_string(&mut buff).unwrap();
    let sun_happenings: Vec<SunHappening> = serde_json::from_str(&buff).unwrap();

    // make clients
    let client = reqwest::Client::new();
    let json_bin_getter_copy = config.json_bin.clone();
    let client_getter = client.get(&format!("{}/{}", config.json_bin.base_url, config.json_bin.bin_id))
        .header("X-Master-Key", json_bin_getter_copy.master_key)
        .header("X-Client-Key", json_bin_getter_copy.access_key)
        .header("Content-Type", "application/json");
    let json_bin_putter_copy = config.json_bin.clone();
    let client_putter = client.put(&format!("{}/{}", config.json_bin.base_url, config.json_bin.bin_id))
        .header("X-Master-Key", json_bin_putter_copy.master_key)
        .header("X-Client-Key", json_bin_putter_copy.access_key)
        .header("Content-Type", "application/json");

    // return initialization bundle
    (config, sun_happenings, client_getter, client_putter)
}

fn get_now(hour_offset: i64) -> (u32, usize) {
    let offset = Duration::hours(hour_offset);
    let now = Utc::now() + offset;
    let now_seconds =  (now.hour() * 3600) + (now.minute() * 60) + now.second();
    let ordinal = now.ordinal() as usize;
    (now_seconds, ordinal)
}

fn is_daylight(now_seconds: u32, sunrise: f32, sunset: f32) -> bool {
    if now_seconds as f32 > sunrise && (now_seconds as f32) < sunset + 3600.0 {
        return true;
    } else {
        return false;
    }
}

async fn get_door_status(client: Option<RequestBuilder>) -> DoorStatus {
    match client {
        Some(client) => {
            match client.send().await {
                Ok(response) => {
                    match response.json::<DoorResponse>().await {
                        Ok(door_response) => {
                            if door_response.record.direction == "up" && door_response.record.executed == true {
                                return DoorStatus::Open;
                            } else if door_response.record.direction == "down" && door_response.record.executed == true {
                                return DoorStatus::Closed;
                            } else if door_response.record.direction == "up" && door_response.record.executed == false {
                                return DoorStatus::Opening;
                            } else if door_response.record.direction == "down" && door_response.record.executed == false {
                                return DoorStatus::Closing;
                            } else {
                                return DoorStatus::Unknown;
                            }
                        },
                        Err(e) => {
                            error!("get_door_status() error parsing json: {}", e);
                            return DoorStatus::Unknown;
                        }
                    }
                },
                _ => {
                    error!("get_door_status() no response");
                    return DoorStatus::Unknown;
                }
            }
        },
        None => {
            error!("get_door_status() no client");
            return DoorStatus::Unknown;
        }
    }
}

fn suggest_action(door_status: DoorStatus, is_daylight: bool) -> DoorAction {
    match door_status {
        DoorStatus::Open => {
            if is_daylight {
                return DoorAction::Pass;
            } else {
                return DoorAction::Close;
            }
        },
        DoorStatus::Closed => {
            if is_daylight {
                return DoorAction::Open;
            } else {
                return DoorAction::Pass;
            }
        },
        _ => {
            warn!("caught door in other than open or closed state");
            return DoorAction::Pass;
        }
    }
}

async fn politely_carry_out_suggestion(suggestion: DoorAction, client: Option<RequestBuilder>) -> String {
    match client {
        Some(client) => {
            match suggestion {
                DoorAction::Open => {
                    let door_record = DoorRecord {
                        direction: "up".to_string(),
                        amount: 10,
                        executed: false
                    };

                    let door_request_json = match serde_json::to_string(&door_record) {
                        Ok(door_request_json) => door_request_json,
                        Err(e) => {
                            error!("carry_out() parse fail on Open: {}", e);
                            return "Error".to_string();
                        }
                    };

                    let response = client.body(door_request_json).send().await;

                    match response {
                        Ok(_) => {
                            info!("Opened");
                            return "Opened".to_string();
                        },
                        Err(e) => {
                            error!("carry_out() response fail on Open: {}", e);
                            return "Error".to_string();
                        }
                    }
                },
                DoorAction::Close => {
                    let door_record = DoorRecord {
                        direction: "down".to_string(),
                        amount: 10,
                        executed: false
                    };

                    let door_request_json = match serde_json::to_string(&door_record) {
                        Ok(door_request_json) => door_request_json,
                        Err(e) => {
                            error!("carry_out() parse fail on Close: {}", e);
                            return "Error".to_string();
                        }
                    };

                    let response = client.body(door_request_json).send().await;

                    match response {
                        Ok(_) => {
                            info!("Closed");
                            return "Closed".to_string();
                        },
                        Err(e) => {
                            error!("carry_out() response fail on Close: {}", e);
                            return "Error".to_string();
                        }
                    }
                },
                DoorAction::Pass => {
                    return "Pass".to_string();
                }
            }
        },
        None => {
            error!("carry_out() no client fail");
            return "No client".to_string();
        }
    }
}