use chrono::{TimeDelta, Utc};
use clap::{App, Arg};
use parse_link_header::parse_with_rel;
use reqwest::*;
use serde::Deserialize;
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Debug, Deserialize)]
struct DemodData {
    payload_demod: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Observation {
    id: i32,
    start: Option<String>,
    end: Option<String>,
    ground_station: Option<i32>,
    transmitter: Option<String>,
    norad_cat_id: Option<i32>,
    payload: Option<String>,
    waterfall: Option<String>,
    demoddata: Vec<DemodData>,
    station_name: Option<String>,
    station_lat: Option<f64>,
    station_lon: Option<f64>,
    station_alt: Option<i32>,
    vetted_status: Option<String>,
    vetted_user: Option<i32>,
    vetted_datetime: Option<String>,
    archived: Option<bool>,
    archive_url: Option<String>,
    client_version: Option<String>,
    client_metadata: Option<String>,
    status: Option<String>,
    waterfall_status: Option<String>,
    waterfall_status_user: Option<i32>,
    waterfall_status_datetime: Option<String>,
    rise_azimuth: Option<f64>,
    set_azimuth: Option<f64>,
    max_altitude: Option<f64>,
    transmitter_uuid: Option<String>,
    transmitter_description: Option<String>,
    transmitter_type: Option<String>,
    transmitter_uplink_low: Option<i32>,
    transmitter_uplink_high: Option<i32>,
    transmitter_uplink_drift: Option<i32>,
    transmitter_downlink_low: Option<i32>,
    transmitter_downlink_high: Option<i32>,
    transmitter_downlink_drift: Option<i32>,
    transmitter_mode: Option<String>,
    transmitter_invert: Option<bool>,
    transmitter_baud: Option<f64>,
    transmitter_updated: Option<String>,
    transmitter_status: Option<String>,
    tle0: Option<String>,
    tle1: Option<String>,
    tle2: Option<String>,
    center_frequency: Option<i32>,
    observer: Option<String>,
    observation_frequency: Option<i32>,
    transmitter_unconfirmed: Option<bool>,
}

impl Observation {
    fn name(&self) -> String {
        match self.start.as_ref() {
            Some(start) => start.clone(),
            None => self.id.to_string(),
        }
    }
}

struct Satellite {
    name: &'static str,
    id: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = App::new("satnogs_downloader")
        .version("0.1.0")
        .author("Patrick Oppel")
        .about("Downloads satnogs observations")
        .arg(Arg::with_name("start_date")
            .short('s')
            .long("start_date")
            .value_name("START_DATE")
            .help("Start date")
            .takes_value(true))
        .arg(Arg::with_name("end_date")
            .short('e')
            .long("end_date")
            .value_name("END_DATE")
            .help("End date")
            .takes_value(true))
        .get_matches();

    let sats = vec!(
        Satellite { name: "CUAVA-2", id: "60527" },
        Satellite { name: "WS-1", id: "60469" },
    );

    let start_date = arguments.value_of("start_date").unwrap_or("2024-08-16");
    let default_end_date = format!("{}", Utc::now().add(TimeDelta::days(1)).format("%F"));
    let end_date = arguments.value_of("end_date").unwrap_or(&default_end_date);
    let api_token = env::var("SATNOGS_API_TOKEN").expect("Provide SATNOGS_API_TOKEN env var");

    for sat in sats {
        // create a new client for each satellite to avoid cursor problems on the server
        let client = Client::new();

        let sat_folder = PathBuf::from(format!("download/{}", sat.name.to_lowercase()));
        fs::create_dir_all(&sat_folder).expect("Unable to create folder");

        let params = format!("start={}&end={}&satellite__norad_cat_id={}&status=good&format=json",
                             start_date, end_date, sat.id);
        let mut url = format!("https://network.satnogs.org/api/observations/?{}", params);

        loop {
            println!("{}: Querying API: {}", sat.name, url);
            let next = download_observations(&client, &sat, &sat_folder, &api_token, &url).await?;
            match next {
                Some(next) => url = next,
                None => break,
            }
        }
    }
    Ok(())
}

async fn download_observations(client: &Client, sat: &Satellite, sat_folder: &PathBuf, api_token: &str, url: &str) -> Result<Option<String>> {
    let response = client.get(url)
        .header(header::AUTHORIZATION, format!("Bearer: {}", api_token))
        .send()
        .await?;

    let next = find_next_url(&response);

    let text = response.text().await?;
    let observations: Vec<Observation> = serde_json::from_str(&text).unwrap();

    for obs in observations {
        if obs.demoddata.is_empty() {
            // Nothing to download!
            continue;
        }

        println!("{}: Downloading observation: {} ({})", sat.name, obs.name(), obs.id);
        let url_file = output_file(&sat_folder, &sat, &obs, "url");
        let data_file = output_file(&sat_folder, &sat, &obs, "raw");

        for demod_data in obs.demoddata {
            fs::write(&url_file, format!("{:?}\n", demod_data.payload_demod).as_bytes())
                .expect("Unable to write data");

            let response = client.get(&demod_data.payload_demod).send().await?;
            fs::write(&data_file, response.bytes().await?.as_ref()).expect("Unable to write data");
        }
    }

    Ok(next)
}

fn find_next_url(response: &Response) -> Option<String> {
    let links = response.headers().get("Link")?;
    match parse_with_rel(links.to_str().unwrap()) {
        Ok(links) => {
            let next_link = links.get("next")?;
            Some(next_link.raw_uri.to_string())
        },
        Err(_) => None
    }
}

fn output_file(parent: &Path, sat: &Satellite, obs: &Observation, suffix: &str) -> PathBuf {
    let filename = format!("{}-{}-beacon.{}", obs.name(), sat.name.to_lowercase(), suffix);
    let mut result = PathBuf::from(parent);
    result.push(filename);
    result
}
