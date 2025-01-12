use crate::{config::Config, door::DoorStatus};

use chrono::{Datelike, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Write},
};
use tracing::{event, Level};

#[derive(Serialize, Deserialize)]
struct SunCouplet {
    sunrise: f32,
    sunset: f32,
}

pub fn update_status_file(config: &Config) {
    // log warning if not updating (executed is 0)
    let status_file = File::open(&config.status_file).expect("Missing status file");
    let status_reader = BufReader::new(status_file);
    let mut door_status: DoorStatus =
        serde_json::from_reader(status_reader).expect("Bad door status structure");

    let (now_seconds, today_idx) = get_now(config.hour_offset);
    let schedule_file = File::open(&config.schedule_file).expect("Missing schedule file");
    let schedule_reader = BufReader::new(schedule_file);
    let sun_couplets: Vec<SunCouplet> =
        serde_json::from_reader(schedule_reader).expect("Bad sun couplet read");
    let todays_couplet = sun_couplets
        .get(today_idx as usize)
        .expect("Bad schedule indexing");

    // unsset override if day has lapsed;
    if door_status.over_ride == 1 {
        if door_status.over_ride_day != today_idx {
            door_status.over_ride = 0;
        }
    }

    // set override day to today
    door_status.over_ride_day = today_idx;

    // set signal for door to raise if conditions are right
    match is_daylight(now_seconds, todays_couplet.sunrise, todays_couplet.sunset) {
        true => {
            if door_status.up == 0 {
                // needs to raise
                if door_status.executed == 0 {
                    event!(Level::WARN, "The door should have been opened by now");
                } else {
                    if door_status.over_ride == 0 {
                        door_status.up = 1;
                        door_status.executed = 0;
                    }
                }
            }
        }
        false => {
            if door_status.up == 1 {
                // needs to lower
                if door_status.executed == 0 {
                    event!(Level::WARN, "The door should have been closed by now");
                } else {
                    if door_status.over_ride == 0 {
                        door_status.up = 0;
                        door_status.executed = 0;
                    }
                }
            }
        }
    }

    // write status to file
    let mut status_file = OpenOptions::new()
        .write(true)
        .create(false)
        .append(false)
        .open(&config.status_file)
        .expect("Missing status file");

    let status_string = serde_json::to_string(&door_status).expect("Couldn't stringify payload");

    status_file
        .write_all(status_string.as_bytes())
        .expect("File write error");
}

fn get_now(hour_offset: i64) -> (u32, u16) {
    let offset = Duration::hours(hour_offset);
    let now = Utc::now() + offset;
    let now_seconds = (now.hour() * 3600) + (now.minute() * 60) + now.second();
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
