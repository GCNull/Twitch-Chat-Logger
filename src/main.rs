use std::collections::HashMap;
use std::env;
use std::error::Error;
// use std::fs::{read_to_string};
use std::io::{BufReader, BufWriter, Write, BufRead};
use std::net::{Shutdown, TcpStream};
use std::process;
use std::thread;
use std::time::*;

use chrono::{NaiveDate, NaiveDateTime, prelude::*};
use postgres::{Client, NoTls};
use rand::{Rng, thread_rng};
use termion::color;
use std::net::Shutdown::Both;
use std::fs::{OpenOptions, File, remove_file};
use std::path::Path;

const BOT_VERSION: &str = env!("CARGO_PKG_VERSION");
static mut CHANNEL: String = String::new();

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
        Ok(socket) => unsafe {
            println!("Chat logger {} running in {}", BOT_VERSION, CHANNEL);

            let mut rng = thread_rng();
            let mut stream =  BufReader::new(&socket);
            let mut wstream = BufWriter::new(&socket);
            let mut buff = String::new();
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/tags").unwrap_or_else(|err|{error_reporter(err);});
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/commands").unwrap_or_else(|err|{error_reporter(err);});
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/membership").unwrap_or_else(|err|{error_reporter(err);});
            send_raw_message(&mut wstream, format!("NICK justinfan{}", rng.gen_range(10000000..99999999)).as_str()).unwrap_or_else(|err|{error_reporter(err);});
            send_raw_message(&mut wstream, format!("JOIN #{}", channel).as_str()).unwrap_or_else(|err|{error_reporter(err);});

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
                    println!("[{}] [{}] {}[{}]: {}", channel, Local::now().format("%T %d/%m/%G").to_string(), raw_user, user["user-id"], raw_message);

                    match Client::connect(&format!("postgresql://postgres:postgres@localhost:5432/{}", &CHANNEL), NoTls) {
                        Ok(mut conn) => {
                            let trans_pid = conn.query("select pid, state, usename, query, query_start from pg_stat_activity where pid in (select pid from pg_locks l join pg_class t on l.relation = t.oid and t.relkind = 'r' where t.relname = 'messages')", &[]).unwrap();
                            if trans_pid.is_empty() {
                                let stmt = conn.prepare("INSERT INTO messages (date, username, user_id, message) VALUES ($1, $2, $3, $4)")?;
                                if Path::new("queued_messages.txt").exists() {
                                    let queued_messages_file = File::open("queued_messages.txt")?;
                                    let read = BufReader::new(queued_messages_file);
                                    for i in read.lines() {
                                        let line = i.unwrap_or_default();
                                        let split: Vec<_> = line.split(" ").collect();
                                        println!("{}From queue: {:?}{}", color::Fg(color::Green), split, color::Fg(color::Reset));
                                        let date = NaiveDate::parse_from_str(split[0], "%Y-%m-%d").unwrap();
                                        let time = NaiveTime::parse_from_str(split[1], "%T").unwrap();
                                        let nt: NaiveDateTime = NaiveDateTime::new(date, time); // 2021-06-25 00:00:00
                                        match conn.execute(&stmt, &[&nt, &split[2], &split[3], &split[4]]) {
                                            Ok(_) => {}
                                            Err(e) => {
                                                println!("{:?}", e);
                                                socket.shutdown(Both).unwrap_or_default();
                                            }

                                        }
                                    }
                                    remove_file("queued_messages.txt").unwrap_or_else(|err| error_reporter(err));
                                }
                                let nt: NaiveDateTime = NaiveDate::from_ymd(Local::now().format("%Y").to_string().parse::<i32>().unwrap(), Local::now().format("%m").to_string().parse::<u32>().unwrap(), Local::now().format("%d").to_string().parse::<u32>().unwrap()).and_hms(Local::now().format("%H").to_string().parse::<u32>().unwrap(), Local::now().format("%M").to_string().parse::<u32>().unwrap(), Local::now().format("%S").to_string().parse::<u32>().unwrap());
                                match conn.execute(&stmt, &[&nt, &raw_user, &user["user-id"], &raw_message]) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("Error 1 writing to db: {:?}\nAdding message to queue and restarting...\n", e);
                                        let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                                        println!("{}Queuing: {}{}", color::Fg(color::Yellow), message_to_queue, color::Fg(color::Reset));
                                        let mut wfile = OpenOptions::new().create(true).append(true).open("queued_messages.txt").unwrap();
                                        wfile.write(format!("{}\n", message_to_queue).as_bytes()).unwrap();
                                        socket.shutdown(Both).unwrap_or_default();
                                        break;
                                    }
                                }
                                conn.close()?;

                            } else {
                                println!("Messages table is busy adding message to queue...");
                                let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                                println!("{}Queuing: {}{}", color::Fg(color::Yellow), message_to_queue, color::Fg(color::Reset));
                                let mut wfile = OpenOptions::new().create(true).append(true).open("queued_messages.txt").unwrap();
                                wfile.write(format!("{}\n", message_to_queue).as_bytes()).unwrap();
                            }

                        }
                        Err(e) => {
                            eprintln!("Error 2 writing to db: {:?}\nAdding message to queue and restarting...", e);
                            let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                            println!("{}Queuing: {}{}", color::Fg(color::Yellow), message_to_queue, color::Fg(color::Reset));
                            let mut wfile = OpenOptions::new().create(true).append(true).open("queued_messages.txt").unwrap();
                            wfile.write(format!("{}\n", message_to_queue).as_bytes()).unwrap();
                        }
                    }
                }

                if buffer.contains("PING :tmi.twitch.tv") {
                    send_raw_message(&mut wstream, "PONG :tmi.twitch.tv").unwrap_or_else(|err|{error_reporter(err);});
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

    fn send_raw_message<W: Write>(w: &mut W, msg: &str) -> Result<(), std::io::Error> {
        let message = format!("{}\r\n", msg);
        if w.write(message.as_bytes()).is_ok() {
            if w.flush().is_ok() {}
            else {
                eprintln!("Failed to send raw data")
            }
        } else {
            eprintln!("Failed to write raw data into buffer");
        }
        Ok(())
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

unsafe fn create_database() -> Result<(), Box<dyn Error>> {
    process::Command::new("sh").arg("scripts/create_db.sh").arg(CHANNEL.to_string()).spawn()?.wait()?;
    let mut conn = Client::connect(&format!("postgresql://postgres:postgres@localhost:5432/{}", &CHANNEL), NoTls).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS messages(
                    date TIMESTAMP WITHOUT TIME ZONE,
                    username VARCHAR(40),
                    user_id VARCHAR(30),
                    message VARCHAR(700) NOT NULL);", &[])?;
    conn.close()?;
    Ok(())
}

pub(crate) fn main() {
    std::process::Command::new("clear").status().unwrap();
    let config = Config::new(env::args()).unwrap_or_else(|err| {
        eprintln!("{}Error: {}{}", color::Fg(color::Red), err, color::Fg(color::Reset));
        process::exit(1);
    });

    unsafe {
        CHANNEL = config.channel;
        create_database().unwrap_or_else(|err| {
            println!("Failed to setup database for {}:\n{:?}", CHANNEL, err);
            process::exit(1);
        });
    }

    loop {
        unsafe {
            bot(CHANNEL.to_string()).unwrap_or_else(|err| {
                eprintln!("{}{}{}", color::Fg(color::Red), err, color::Fg(color::Reset))});
            println!("Bot function ending. Attempting to repeat loop...");
            sleep(50);
        }
    }
}
