use std::fs::File;
use std::io::Read;
use serde::Deserialize;
use serde_json;

#[derive(Deserialize)]
struct SunHappening {
    sunrise: f32,
    sunset: f32,
}


fn main() {
    // import JSON file of sunrise/sunset times for every day of the year
    let mut file = File::open("files/schedule.json").unwrap();
    let mut buff = String::new();
    file.read_to_string(&mut buff).unwrap();
    let mut sun_times: Vec<SunHappening> = serde_json::from_str(&buff).unwrap();

    println!("{:?}", sun_times[0].sunrise);


    println!("Hello, world!");
}
