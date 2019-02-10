extern crate ini;
extern crate curl;
extern crate core;

use ini::Ini;
use curl::easy::Easy;
use std::collections::HashMap;
use std::str;
use std::io::{Write};

static DEBUG: bool = true;

#[derive(Debug)]
#[derive(PartialEq)]
enum State {
    Up,
    Down,
    Unknown,
}

#[derive(Debug)]
struct StateCounter {
    state: State,
    count: u64,
    notified: bool,
}

#[derive(Debug)]
struct Config {
    interval: u64,
    identifier: String,
    slack_url: String,
    sites: Vec<String>,
    max_retries: u64,
}

fn main() {

    let config = read_config(
        "./check-websites.conf"
    );

    // Create a HashMap to keep count of states.
    let mut site_states:HashMap<String, StateCounter> = HashMap::new();


    for site in &config.sites {
        let mut statecounter = StateCounter{state: State::Unknown, count:0, notified: true};
        site_states.insert(site.clone(), statecounter);
    }

    // Loop through all the sites, defining state with a cURL
    // action. Parse the header if given and check for correct
    // working of the site (2xx, 3xx). Register site Up/Down.
    loop {
        if DEBUG { eprint!("."); }
        std::io::stdout().flush().unwrap();

        for site in &config.sites {
            let current_state = get_site_state(&site.clone());
            let state_counter = site_states.get_mut(&site.clone()).unwrap();


            if current_state == State::Down {
                if DEBUG { eprint!("x({})", state_counter.count); }
                state_counter.state = State::Down;
                state_counter.count += 1;
            }

            if state_counter.count == config.max_retries {
                if DEBUG {
                    eprintln!("{} is Down ({} seconds) :(", site, config.interval * state_counter.count);
                }
                std::io::stdout().flush().unwrap();
                continue;
            }

            if current_state == State::Up && state_counter.count < config.max_retries {
                state_counter.state = State::Up;
                state_counter.count = 0;
                continue;
            }
            if current_state == State::Up && state_counter.count > config.max_retries{

                if DEBUG {
                    eprintln!("Site {} is Up again after {} seconds!", site, config.interval * state_counter.count);
                }
                std::io::stdout().flush().unwrap();
                state_counter.count = 0;
            }

        }
        std::thread::sleep(std::time::Duration::from_secs(config.interval));

    }
}

fn get_site_state(url: &str) -> State{
    let mut headers = Vec::new();
    let mut handle = Easy::new();
    let mut state = State::Unknown;

    handle.url(&url).unwrap();

    if let Err(e) = handle.ssl_verify_peer(false) {
        println!("Could not set ssl_verify_peer to false: {}", e);
    }

    {
        let mut transfer = handle.transfer();
        transfer.header_function(|header| {
            headers.push(str::from_utf8(header).unwrap().to_string());
            true
        }).unwrap();
        match transfer.perform() {
            Ok(r) => {
                r
            },
            Err(e) => {
                println!("{}: {}",url,e);

                // Site is unreachable so register as Down
                state = State::Down;
            }
        };
    }

    if headers.len() > 0 {

        let code_2xx = headers[0].find("HTTP/1.1 2").unwrap_or(1);
        let code_3xx = headers[0].find("HTTP/1.1 3").unwrap_or(1);

        if  code_2xx == 0 ||  code_3xx== 0 {
            state = State::Up;
        }
        else {
            state = State::Down;
        }
    }

    state
}


fn read_config(filename: &str) -> Config {

    let conf = Ini::load_from_file(filename).unwrap();
    let settings = conf.section(
        Some("settings".to_owned())
    ).expect("Could not read section [settings] from config file.");

    let split = settings.get("sites").unwrap().split(" ");
    let mut sites: Vec<String> = Vec::new();

    for site in split {
        if site != "" {
            sites.push(String::from(site));
        }
    }

    let config: Config = Config {
        interval: settings.get("interval").unwrap().parse().unwrap(),
        identifier: settings.get("identifier").unwrap().parse().unwrap(),
        slack_url: settings.get("slack_url").unwrap().parse().unwrap(),
        max_retries: settings.get("max_retries").unwrap().parse().unwrap(),
        sites
    };
    config

}
