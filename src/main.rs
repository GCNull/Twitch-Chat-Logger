use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::{OpenOptions, read_to_string, remove_file};
use std::io::{BufReader, BufWriter, Write};
use std::net::{Shutdown, TcpStream};
use std::process;
use std::thread;
use std::time::*;

use chrono::prelude::*;
use postgres::{Client, NoTls};
use rand::{Rng, thread_rng};
// use serde_json::Value;
use termion::color;

const BOT_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Config {
    channel: String,
}

impl Config {
    fn new(mut args: env::Args) -> Result<Config, &'static str> {
        args.next();

        let channel = match args.next() {
            Some(arg) => arg,
            None => return Err("Usage: <channel>"),
        };
        Ok(Config {
            channel,
        })
    }
}

fn sleep(x: u64) {
    thread::sleep(Duration::from_millis(x));
}

fn error_reporter(err: std::io::Error) {
    eprintln!("{}{}{}", color::Fg(color::Red), err, color::Fg(color::Reset));
}

// fn url_fetch(link: &str) -> Result<String,  reqwest::Error> {
//     let result = reqwest::blocking::get(link)?.text().unwrap();
//     Ok(result)\
// }
//
// fn json_parser(json: String) -> Result<Value, Box<dyn Error>> {
//     let json_d: serde_json::Value = serde_json::from_str(&json)?;
//     Ok(json_d)
// }

fn bot(channel: String) -> Result<(), Box<dyn Error>> {
    match TcpStream::connect("irc.chat.twitch.tv:6667") {
        Ok(socket) => {
            let mut message_queue: Vec<String> = Vec::new();
            println!("Chat logger {} running", BOT_VERSION);

            let mut rng = thread_rng();
            let mut stream =  BufReader::new(&socket);
            let mut wstream = BufWriter::new(&socket);
            let mut buff = String::new();
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/tags");
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/commands");
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/membership");
            send_raw_message(&mut wstream, format!("NICK justinfan{}", rng.gen_range(10000000..99999999)).as_str());
            send_raw_message(&mut wstream, format!("JOIN #{}", channel).as_str());

            while std::io::BufRead::read_line(&mut stream, &mut buff)? > 0 {
                let buffer = buff.trim();
                if buffer.contains(":tmi.twitch.tv CAP * ACK :twitch.tv/tags") {
                    println!("Tags request acknowledged")
                } else if buffer.contains(":tmi.twitch.tv CAP * ACK :twitch.tv/commands") {
                    println!("Commands request acknowledged")
                } else if buffer.contains(":tmi.twitch.tv CAP * ACK :twitch.tv/membership") {
                    println!("Membership request acknowledged")
                }   else if buffer.contains(":Welcome, GLHF!") {
                    println!("\n[{}]\nConnected to Twitch IRC\n", Local::now().format("%H:%M:%S %d/%b/%Y").to_string());
                }

                if buffer.contains("PRIVMSG") {
                    let channel: Vec<&str> = buffer.split(" ").collect();
                    let channel = channel[3];
                    let user = extract_tags(&buffer);
                    let raw_user: Vec<&str> = user["user-type"].split(|c| c == '!' || c == '@').collect();
                    let raw_user = raw_user[1];
                    let raw_message = buffer.rsplit(format!("{} :", channel).as_str()).next().unwrap().trim().to_string();
                    let raw_user = raw_user.to_string();
                    let user_id = user["user-id"].to_string();
                    // println!("[{}] [{}] <{}>[{}]: {}", channel, Local::now().format("%T %d/%m/%G").to_string(), raw_user, user["user-id"], raw_message);

                    match Client::connect(&get_db_channel(), NoTls) {
                        Ok(mut conn) => {
                            let stmt = conn.prepare("INSERT INTO messages (date, username, user_id, message) VALUES ($1, $2, $3, $4)")?;

                            if !message_queue.is_empty() {
                                for i in message_queue.iter() {
                                    let split: Vec<_> = i.split_whitespace().collect();
                                    let dt = format!("{} {}", split[0], split[1]);
                                    println!("From queue: {}", i);
                                    conn.execute(&stmt, &[&dt, &split[2], &split[3], &split[4]])?;
                                }
                                message_queue.clear();
                            }

                            if conn.execute(&stmt, &[&Local::now().format("%Y-%m-%d %T").to_string(), &raw_user, &user["user-id"], &raw_message]).is_ok() {
                            } else {
                                eprintln!("Errorrrrrrrrrrrrrrrrrrrr writing to db. Adding message to queue and restarting...\n");
                                // message_queue.push(RAW_MESSAGE.to_string());
                            }
                            conn.close()?;
                        }
                        Err(e) => {
                            eprintln!("Error writing to db: {:?}\nAdding message to queue and restarting...", e);
                            let message_to_queue = format!("{} {} {} {}", Local::now().format("%Y-%m-%d %T").to_string(), raw_user, user_id, raw_message);
                            println!("{}", message_to_queue);
                            message_queue.push(message_to_queue);
                        }
                    }
                }

                if buffer.contains("PING :tmi.twitch.tv") {
                    send_raw_message(&mut wstream, "PONG :tmi.twitch.tv");
                    println!("[chat_logger.rs] PONG at {}",Local::now().format("%T %d/%m/%G").to_string());
                }
                buff.clear();
            }
            if std::io::BufRead::read_line(&mut stream, &mut buff).unwrap() == 0 {
                socket.shutdown(Shutdown::Write).unwrap_or_else(|err| eprintln!("Failed to shutdown socket: {}", err));
                eprintln!("\n{}Socket disconnected{}", color::Fg(color::Red), color::Fg(color::Reset));
            }
        }
        Err(e) => error_reporter(e),
    };

    fn send_raw_message<W: Write>(w: &mut W, msg: &str) {
        let message = format!("{}\r\n", msg);
        w.write(message.as_bytes()).expect("Failed to write message into the buffer");
        w.flush().expect("Failed to send message");
        // print!("Sent: {}", message);
    }

    fn extract_tags(tags: &str) -> HashMap<String, String> {
        let irc_tags = tags.trim_start_matches('@').trim_end_matches("PRIVMSG");
        irc_tags.split(";").flat_map(|tag| {
            let mut split = tag.splitn(2, "=");
            let key = split.next()?.to_owned();
            let value = split.next()?.to_owned();
            Some((key, value))
        }).collect()
    }
    Ok(())
}

fn get_db_channel() -> String {
    let rfile = read_to_string(".env").unwrap();
    rfile
}

fn create_database(cc: &str) -> Result<(), Box<dyn Error>> {
    process::Command::new("sh").arg("scripts/create_db.sh").arg(cc).spawn()?.wait()?;
    let mut conn = Client::connect(&get_db_channel(), NoTls).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS messages(
                    id SERIAL PRIMARY KEY,
                    date VARCHAR(25),
                    username VARCHAR(40),
                    user_id VARCHAR(30),
                    message VARCHAR(700) NOT NULL);", &[])?;
    conn.close()?;
    Ok(())
}

fn main() {
    std::process::Command::new("clear").status().unwrap();
    let config = Config::new(env::args()).unwrap_or_else(|err| {
        eprintln!("{}Error: {}{}", color::Fg(color::Red), err, color::Fg(color::Reset));
        process::exit(1);
    });

    create_database(&config.channel).unwrap_or_else(|err| {
        println!("Failed to setup database for {}:\n{:?}", config.channel, err);
        process::exit(1);
    });

    loop {
        remove_file(".env").unwrap_or_else(|err| {
            if err.to_string().to_lowercase().contains("no such file") {println!(".env file not found. Continuing...")}});
        let mut wfile = OpenOptions::new().create(true).append(true).open(".env").unwrap();

        wfile.write(format!("postgresql://postgres:postgres@localhost:5432/{}", config.channel).as_bytes()).unwrap();

        bot(config.channel.to_string()).unwrap_or_else(|err| {
            eprintln!("{}{}{}", color::Fg(color::Red), err, color::Fg(color::Reset))});
        println!("Bot function ending. Attempting to repeat loop...");
        sleep(50);
    }
}
