use std::fs::File;
use std::io::Read;
use std::thread;
use std::time;

use axum::{http::{header::HeaderMap, Method, StatusCode}, routing::{get, put}, Router, Json, Extension};
use chrono::{Datelike, Duration, Local, Timelike, Utc};
use fern;
use log::{info, warn, error};
use serde::{Serialize, Deserialize};
use serde_json;
use sqlx::{Sqlite, SqlitePool, migrate::MigrateDatabase, Row};
use tokio;
use toml;
use tower_http::cors::{Any, CorsLayer};

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
    executed: u8,
    up: u8,
    amount: u8,
    over_ride: Option<u8>,
    over_ride_day: Option<u16>,
}

#[derive(Deserialize, Clone)]
struct Config {
    interval_seconds: u64,
    hour_offset: i64,
    access_key: String,
}

const DB_URL: &str = "sqlite://db/chicken_door.db";

#[tokio::main]
async fn main() {

    let (config, sun_happenings) = initialize().await;
    let config_clone = config.clone();

    let pool = match SqlitePool::connect(DB_URL).await {
        Ok(pool) => {
            info!("connected to db (main)");
            pool
        },
        Err(error) => {
            error!("cannot connect to db (main): {}", error);
            panic!("error: {}", error);
        }
    };

    info!("entering main loop");

    thread::spawn(move || {
        loop {
            let (now_seconds, ordinal) = get_now(config.hour_offset);
            let is_daylight = is_daylight(now_seconds, sun_happenings[ordinal as usize].sunrise, sun_happenings[ordinal as usize].sunset);
            
            tokio::runtime::Runtime::new().unwrap().handle().block_on(async {
                let door_status = get_door_status(&pool).await;
                let suggestion = suggest_action(door_status, is_daylight, &pool, ordinal).await;
                politely_carry_out_suggestion(suggestion, &pool).await;
            });

            let interval_seconds = time::Duration::from_secs(config.interval_seconds);
            thread::sleep(interval_seconds);
        }; 
    });

    let app = Router::new()
        .route("/", get(|| async { "oh, Chicken, Chicken, you can't roost too high for me" }))
        .route("/get_door_status", get(get_req_door_status))
        .route("/update_door_status", put(update_req_door_status))
        .layer(Extension(config_clone))
        .layer(
            CorsLayer::new()
                .allow_methods([Method::GET, Method::PUT])
                .allow_origin(Any)
        );

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();

}

async fn get_req_door_status(Extension(config): Extension<Config>, headers: HeaderMap) -> Result<Json<DoorRecord>, StatusCode> {
    info!("GET | /get_door_status");

    let access_pass = match headers.get("x-access-key") {
        Some(access_pass) => access_pass,
        None => {
            error!("no access key provided");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    let access_pass_string = match access_pass.to_str() {
        Ok(access_pass_string) => access_pass_string,
        Err(error) => {
            error!("error converting access key to string: {}", error);
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if config.access_key == access_pass_string {
        let pool = match SqlitePool::connect(DB_URL).await {
            Ok(pool) => {
                info!("connected to db (get_req_door_status)");
                pool
            },
            Err(error) => {
                error!("cannot connect to db (get_req_door_status): {}", error);
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        };
    
        // retrieve first entry from db
        let row = match sqlx::query("SELECT * FROM door_status ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool).await {
            Ok(row) => row,
            Err(error) => {
                error!("error retrieving row from db (get_door_status): {}", error);
                return Err(StatusCode::UNPROCESSABLE_ENTITY);
            }
        };
          
        let columns = vec!["executed", "up", "amount", "over_ride", "over_ride_day"];
    
        let column_vals: Vec<u16> = columns.into_iter().map(|col_name| row.get::<u16, &str>(col_name)).collect();
    
        let door_record = DoorRecord {
            executed: column_vals[0] as u8,
            up: column_vals[1] as u8,
            amount: column_vals[2] as u8,
            over_ride: Some(column_vals[3] as u8),
            over_ride_day: Some(column_vals[4]),
        };
    
        return Ok(Json(door_record));
    } else {
        warn!("unauthorized access attempt to (get_req_door_status)");
        return Err(StatusCode::UNAUTHORIZED);
    }

    
}

async fn update_req_door_status(Extension(config): Extension<Config>, headers: HeaderMap, Json(door_record): Json<DoorRecord>) -> Result<Json<DoorRecord>, StatusCode> {
    info!("PUT | /update_door_status");

    let access_pass = match headers.get("x-access-key") {
        Some(access_pass) => access_pass,
        None => {
            error!("no access key provided");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    let access_pass_string = match access_pass.to_str() {
        Ok(access_pass_string) => access_pass_string,
        Err(error) => {
            error!("error converting access key to string: {}", error);
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    if config.access_key == access_pass_string {

        let pool = match SqlitePool::connect(DB_URL).await {
            Ok(pool) => {
                info!("connected to db (update_req_door_status)");
                pool
            },
            Err(error) => {
                error!("cannot connect to db (update_req_door_status): {}", error);
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        };

        // add entry to pool
        match door_record.over_ride {
            Some(1) => {
                info!("updating door status with override 1");
                let (_, ordinal) = get_now(config.hour_offset);
                let payload = format!("{},{},{},{},{}", door_record.executed, door_record.up, door_record.amount, 1, ordinal);
                match sqlx::query(
                    &format!("REPLACE INTO door_status (id, executed, up, amount, over_ride, over_ride_day) VALUES (1,{})", payload))
                    .execute(&pool)
                    .await 
                {
                    Ok(_) => {
                        info!("insert success");
                    },
                    Err(error) => {
                        error!("{}", error);
                        return Err(StatusCode::UNPROCESSABLE_ENTITY);
                    }
                }
            },
            Some(0) => {
                info!("updating door status with override 0");
                let (_, ordinal) = get_now(config.hour_offset);
                let payload = format!("{},{},{},{},{}", door_record.executed, door_record.up, door_record.amount, 0, ordinal);
                match sqlx::query(
                    &format!("REPLACE INTO door_status (id, executed, up, amount, over_ride, over_ride_day) VALUES (1,{})", payload))
                    .execute(&pool)
                    .await 
                {
                    Ok(_) => {
                        info!("insert success");
                    },
                    Err(error) => {
                        error!("{}", error);
                        return Err(StatusCode::UNPROCESSABLE_ENTITY);
                    }
                }
            },
            _ => {
                info!("updating door status without override");
                match sqlx::query(
                    &format!("UPDATE door_status SET executed={}, up={}, amount={} WHERE id=1", door_record.executed, door_record.up, door_record.amount))
                    .execute(&pool)
                    .await 
                {
                    Ok(_) => {
                        info!("insert success");
                    },
                    Err(error) => {
                        error!("{}", error);
                        return Err(StatusCode::UNPROCESSABLE_ENTITY);
                    }
                }
            },
        };
        
        return Ok(Json(door_record));
    } else {
        warn!("unauthorized access attempt (update_req_door_status)");
        return Err(StatusCode::UNAUTHORIZED);
    }
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
        .level_for("sqlx", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .chain(fern::log_file("logs/chicken_door.log").unwrap())
        .apply().unwrap();

    // set up database
    if !Sqlite::database_exists(DB_URL).await.unwrap_or(false) {
        info!("creating database (initialize): {}", DB_URL);
        match Sqlite::create_database(DB_URL).await {
            Ok(_) => info!("create db success (initialize)"),
            Err(error) => panic!("create db error (initialize): {}", error),
        }

        let pool = match SqlitePool::connect(DB_URL).await {
            Ok(pool) => {
                info!("connected to db (initialize)");
                pool
            },
            Err(error) => {
                error!("sql pool connect error (initialize): {}", error);
                panic!("error: {}", error);
            }
        };
        
        //run migrations
        match sqlx::query("CREATE TABLE IF NOT EXISTS door_status (id INTEGER PRIMARY KEY NOT NULL, executed INTEGER NOT NULL, up INTEGER NOT NULL, amount INTEGER NOT NULL, over_ride INTEGER NOT NULL, over_ride_day INTEGER NOT NULL);").execute(&pool).await {
            Ok(_) => info!("create table success"),
            Err(error) => {
                error!("sql migration error (initialize): {}", error);
                panic!("error: {}", error);
            }
        }
    
        // add entry to pool
        match sqlx::query("INSERT INTO door_status (executed, up, amount, over_ride, over_ride_day) VALUES (1, 1, 10, 0, 0)").execute(&pool).await {
            Ok(_) => info!("create table success (initialize)"),
            Err(error) => {
                error!("creat table fail (initialize): {}", error);
                panic!("error: {}", error);
            }
        }

    } else {
        info!("db already exists (initialize)");
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

    // return initialization bundle
    (config, sun_happenings)
}

fn get_now(hour_offset: i64) -> (u32, u16) {
    let offset = Duration::hours(hour_offset);
    let now = Utc::now() + offset;
    let now_seconds =  (now.hour() * 3600) + (now.minute() * 60) + now.second();
    let ordinal = now.ordinal() as u16;
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

    let column_vals: Vec<u16> = columns.into_iter().map(|col_name| row.get::<u16, &str>(col_name)).collect();

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

async fn suggest_action(door_status: DoorStatus, is_daylight: bool, pool: &SqlitePool, today: u16) -> DoorAction {
    // retrieve first entry from db
    let row = match sqlx::query("SELECT * FROM door_status ORDER BY id DESC LIMIT 1")
        .fetch_one(pool).await {
        Ok(row) => row,
        Err(error) => {
            error!("get_door_status() error retrieving row from db: {}", error);
            return DoorAction::Pass;
        }
    };
      
    let columns = vec!["executed", "up", "amount", "over_ride", "over_ride_day"];

    let column_vals: Vec<u16> = columns.into_iter().map(|col_name| row.get::<u16, &str>(col_name)).collect();

    if column_vals[3] == 1 && column_vals[4] == today {
        info!("over ride in effect");
        return DoorAction::Pass;
    }

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

#[allow(unused_assignments)]
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
        match sqlx::query(
            &format!("REPLACE INTO door_status (id, executed, up, amount, over_ride, over_ride_day) VALUES (1,{},{})", action_string, "0,0"))
            .execute(pool)
            .await 
        {
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