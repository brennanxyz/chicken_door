use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub struct DoorStatus {
    pub executed: u8,
    pub up: u8,
    pub over_ride: u8,
    pub over_ride_day: u16,
}
