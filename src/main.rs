extern crate ini;
extern crate curl;
extern crate core;
extern crate docopt;
extern crate slack_hook;

#[macro_use] extern crate log;
extern crate env_logger;

use ini::Ini;
use curl::easy::Easy;
use std::collections::HashMap;
use std::str;
use std::io::{Write};
use log::Level;
use slack_hook::{Slack, PayloadBuilder};

//use docopt::Docopt;
//const USAGE: &'static str = r#"
//Check Websites
//
//Usage:
//    check-websites -c <config file> [-d | -f]
//
//Options:
//    -d --daemon     Run in daemon mode
//    -f --foreground Run in foreground, log output to screen  (default)
//    -l <log level>  Specifies log level: debug, info, warning, errror (default)
//    (-h --help)     Show this
//"#;


/// The state describes the state of an website
///
#[derive(Debug)]
#[derive(PartialEq)]
enum State {
    Up,
    Down,
    Unknown,
}

// holds the total count for many times the website has
// been sequentially seen in this state and registers if
// there has been a notification sent since the last state
// switch.
#[derive(Debug)]
struct StateCounter {
    state: State,
    count: u64,
    notified: bool,
}

// The Config structs holds all necessary paramaters that
// have been read from the configuration file.
#[derive(Debug)]
struct Config {
    interval: u64,
    identifier: String,
    slack_url: String,
    sites: Vec<String>,
    max_retries: u64,
}

fn main() {

    env_logger::init();

    let config = read_config(
        "/etc/check-websites.conf"
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
        if log_enabled!(Level::Debug){
            debug!(".");
        }
        std::io::stdout().flush().unwrap();

        for site in &config.sites {
            let current_state = get_site_state(&site.clone());
            let state_counter = site_states.get_mut(&site.clone()).unwrap();

            // Notice that site is down.
            if current_state == State::Down {
                if log_enabled!(Level::Warn)
                {
                    warn!("{} down ({} seconds)", site, config.interval * state_counter.count);
                }
                state_counter.state = State::Down;
                state_counter.count += 1;
            }

            // Site is down and max_retries has expired, so send error message.
            if state_counter.count == config.max_retries {
                if log_enabled!(Level::Error) {
                    // Todo: Make message variable
                    send_slack_message(&config, site, &current_state);
                    state_counter.notified = true;
                }

                std::io::stdout().flush().unwrap();

                // Skip this count number
                state_counter.count += 1;
                continue;
            }

            // Site was down, but is up again before mach retries is reached.
            if current_state == State::Up && state_counter.count < config.max_retries {
                state_counter.state = State::Up;
                state_counter.count = 1;
                continue;
            }

            // Site was down, notice was sent (max_retries expired)
            // so send an 'up again'-message.
            if current_state == State::Up && state_counter.notified {

                if log_enabled!(Level::Error) {
                    send_slack_message(&config, site, &current_state);
                }
                state_counter.notified = false;
                std::io::stdout().flush().unwrap();
                state_counter.count = 1;
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
                error!("{}: {}",url,e);
                // cURL failed to register site as Down
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

fn send_slack_message(config: &Config, site: &String, state: &State) {

    let slack = Slack::new(config.slack_url.as_str()).unwrap();

    let text = match () {
        _ if state == &State::Up => format!("{}: Yey! :tada: :tada: :tada: *{}* is UP Again!", config.identifier, site),
        _ if state == &State::Down => format!("{}: *{}* is DOWN :sob:", config.identifier, site),
        _ => String::from(""),
    };

    if text != "" {
        let p = PayloadBuilder::new()
            .text(text)
            .channel("#monitor")
            .icon_emoji(":chart_with_upward_trend:")
            .build()
            .unwrap();

        let _res = slack.send(&p);
    }
}

fn read_config(filename: &str) -> Config {

    let conf = Ini::load_from_file(filename).unwrap();
    let settings = conf.section(
        Some("settings".to_owned())
    ).expect("Could not read section [settings] from config file.");

    let split = settings.get("sites").unwrap().split(" ");
    let mut sites: Vec<String> = Vec::new();

    info!("--- Configuration ---");
    info!("Checking websites :");
    for site in split {
        if site != "" {
            info!(" {}", site);
            sites.push(String::from(site));
        }
    }
    info!("---");

    let config: Config = Config {
        interval: settings.get("interval").unwrap_or(&String::from("None")).parse().unwrap_or_else(|x|{
            error!("{}",format!("Error parsing configuration: {:?}", x));
            info!("No valid interval found. Setting it to 90 seconds");
            90
        }),
        identifier: settings.get("identifier").unwrap().parse().unwrap(),
        slack_url: settings.get("slack_url").unwrap().parse().unwrap(),
        max_retries: settings.get("max_retries").unwrap().parse().unwrap_or_else(|x|{
            error!("{}",format!("Error parsing configuration: {:?}", x));
            info!("No max_retries value found. Setting it to 5");
            5
        }),
        sites
    };

    info!("Checking with interval: {}", config.interval);
    info!("Max retries: {}", config.max_retries);
    info!("Slack Identifier: {}", config.identifier);
    info!("Slack URL: {}", format!("{}/....../",&config.slack_url.split("/").collect::<Vec<&str>>()[0..6].join("/")));
    info!("---------------------");

    config

}
