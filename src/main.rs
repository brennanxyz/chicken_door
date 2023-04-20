use std::fs::File;
use std::io::Read;
use std::thread;
use std::time;

use axum::{routing::get, Router,};
use chrono::{Datelike, Duration, Local, Timelike, Utc};
use fern;
use log::{info, warn, error};
use serde::{Serialize, Deserialize};
use serde_json;
use sqlx::{Sqlite, SqlitePool, migrate::MigrateDatabase, Row};
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
}

const DB_URL: &str = "sqlite://chicken_door.db";

#[tokio::main]
async fn main() {

    let (config, sun_happenings) = initialize().await;

    info!("entering main loop");

    let pool = match SqlitePool::connect(DB_URL).await {
        Ok(pool) => {
            info!("connected to db");
            pool
        },
        Err(error) => {
            error!("cannot connect to db: {}", error);
            panic!("error: {}", error);
        }
    };

    thread::spawn(move || {
        loop {
            info!("loop hit");
            let (now_seconds, ordinal) = get_now(config.hour_offset);
            let is_daylight = is_daylight(now_seconds, sun_happenings[ordinal].sunrise, sun_happenings[ordinal].sunset);
            
            tokio::runtime::Runtime::new().unwrap().handle().block_on(async {
                let door_status = get_door_status(&pool).await;
                println!("door status: {:?}", door_status);
                let suggestion = suggest_action(door_status, is_daylight);
                politely_carry_out_suggestion(suggestion, &pool).await;
            });

            let interval_seconds = time::Duration::from_secs(config.interval_seconds);
            thread::sleep(interval_seconds);
        }; 
    });

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/door_status", get(door_status));

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();

}

async fn door_status() -> &'static str {
    "door status"
}

async fn initialize() -> (Config, Vec<SunHappening>) {
    // set up logging
    let _log_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{},{},{}",
                Local::now().format("%Y-%m-%d|%H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(fern::log_file("logs/chicken_door.log").unwrap())
        .apply().unwrap();

    // set up database
    if !Sqlite::database_exists(DB_URL).await.unwrap_or(false) {
        info!("creating database {}", DB_URL);
        match Sqlite::create_database(DB_URL).await {
            Ok(_) => info!("create db success"),
            Err(error) => panic!("error: {}", error),
        }

        let pool = match SqlitePool::connect(DB_URL).await {
            Ok(pool) => {
                info!("connected to db");
                pool
            },
            Err(error) => {
                error!("{}", error);
                panic!("error: {}", error);
            }
        };
        
        //run migrations
        match sqlx::query("CREATE TABLE IF NOT EXISTS door_status (id INTEGER PRIMARY KEY NOT NULL, executed INTEGER NOT NULL, up INTEGER NOT NULL, amount INTEGER NOT NULL);").execute(&pool).await {
            Ok(_) => info!("create table success"),
            Err(error) => {
                error!("{}", error);
                panic!("error: {}", error);
            }
        }
    
        // add entry to pool
        match sqlx::query("INSERT INTO door_status (executed, up, amount) VALUES (1, 1, 10)").execute(&pool).await {
            Ok(_) => info!("create table success"),
            Err(error) => {
                error!("{}", error);
                panic!("error: {}", error);
            }
        }

    } else {
        info!("db already exists");
    }
    

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
    // let client = reqwest::Client::new();
    // let json_bin_getter_copy = config.json_bin.clone();
    // let client_getter = client.get(&format!("{}/{}", config.json_bin.base_url, config.json_bin.bin_id))
    //     .header("X-Master-Key", json_bin_getter_copy.master_key)
    //     .header("X-Client-Key", json_bin_getter_copy.access_key)
    //     .header("Content-Type", "application/json");
    // let json_bin_putter_copy = config.json_bin.clone();
    // let client_putter = client.put(&format!("{}/{}", config.json_bin.base_url, config.json_bin.bin_id))
    //     .header("X-Master-Key", json_bin_putter_copy.master_key)
    //     .header("X-Client-Key", json_bin_putter_copy.access_key)
    //     .header("Content-Type", "application/json");

    // return initialization bundle
    (config, sun_happenings)
}

fn get_now(hour_offset: i64) -> (u32, usize) {
    let offset = Duration::hours(hour_offset);
    let now = Utc::now() + offset;
    let now_seconds =  (now.hour() * 3600) + (now.minute() * 60) + now.second();
    let ordinal = now.ordinal() as usize;
    (now_seconds, ordinal)
}

fn is_daylight(now_seconds: u32, sunrise: f32, sunset: f32) -> bool {
    if now_seconds as f32 > sunrise && (now_seconds as f32) < sunset + 1800.0 {
        return true;
    } else {
        return false;
    }
}

async fn get_door_status(pool: &SqlitePool) -> DoorStatus {

    // retrieve first entry from db
    let row = match sqlx::query("SELECT * FROM door_status ORDER BY id DESC LIMIT 1")
        .fetch_one(pool).await {
        Ok(row) => row,
        Err(error) => {
            error!("get_door_status() error retrieving row from db: {}", error);
            return DoorStatus::Unknown;
        }
    };
      
    let columns = vec!["executed", "up", "amount"];

    let column_vals: Vec<u32> = columns.into_iter().map(|col_name| row.get::<u32, &str>(col_name)).collect();

    if column_vals[0] == 1 && column_vals[1] == 1 {
        return DoorStatus::Open;
    } else if column_vals[0] == 1 && column_vals[1] == 0 {
        return DoorStatus::Closed;
    } else if column_vals[0] == 0 && column_vals[1] == 1 {
        return DoorStatus::Opening;
    } else if column_vals[0] == 0 && column_vals[1] == 0 {
        return DoorStatus::Closing;
    } else {
        return DoorStatus::Unknown;
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

async fn politely_carry_out_suggestion(suggestion: DoorAction, pool: &SqlitePool) -> () {

    let mut action_string = String::new();

    match suggestion {
        DoorAction::Open => {
            action_string = "0,1,10".to_string();
        },
        DoorAction::Close => {
            action_string = "0,0,10".to_string();
        },
        DoorAction::Pass => {
            action_string = "pass".to_string();
        }
    }

    // update db
    if action_string != "pass" {
        match sqlx::query(&format!("REPLACE INTO door_status (id, executed, up, amount) VALUES (1,{})", action_string)).execute(pool).await {
            Ok(_) => {
                info!("insert success");
            },
            Err(error) => {
                error!("{}", error);
                panic!("error: {}", error);
            }
        }
    }



}