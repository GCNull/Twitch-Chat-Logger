// #[cfg(not(target_env = "msvc"))]
// use jemallocator::Jemalloc;
//
// #[cfg(not(target_env = "msvc"))]
// #[global_allocator]
// static GLOBAL: Jemalloc = Jemalloc;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::{create_dir, File, OpenOptions, remove_file};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{Shutdown, TcpStream};
use std::net::Shutdown::Both;
use std::path::Path;
use std::process;
use std::thread;
use std::time::*;

use chrono::{NaiveDate, NaiveDateTime, prelude::*};
use postgres::{Client, NoTls};
use rand::{Rng, thread_rng};
use serde_derive::Deserialize;
use serde_json;
use termion::color;
use std::any::Any;

const BOT_VERSION: &str = env!("CARGO_PKG_VERSION");
static mut CHANNEL: String = String::new();

#[derive(Debug, Deserialize)]
struct Config2 {
    username: String,
    password: String,
}

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

fn queue_message(message_to_queue: String) -> Result<(), Box<dyn Error>> {
    let queued_messages_path: String = format!("channels/{}_queued_messages.txt", unsafe { &CHANNEL });
    println!("{}Queuing: {}{}", color::Fg(color::Yellow), message_to_queue, color::Fg(color::Reset));
    let mut wfile = OpenOptions::new().create(true).append(true).open(&queued_messages_path)?;
    wfile.write(format!("{}\n", message_to_queue).as_bytes())?;
    Ok(())
}

fn read_json_from_file<P: AsRef<Path>>(path: P) -> Result<Config2, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let json = serde_json::from_reader(reader)?;
    Ok(json)
}

fn bot(channel: String) -> Result<(), Box<dyn Error>> {
    let queued_messages_path: String = format!("channels/{}_queued_messages.txt", unsafe { &CHANNEL });
    match TcpStream::connect("irc.chat.twitch.tv:6667") {
        Ok(socket) => unsafe {
            println!("Chat logger {} running in {}", BOT_VERSION, CHANNEL);
            let mut rng = thread_rng();
            let mut stream = BufReader::new(&socket);
            let mut wstream = BufWriter::new(&socket);
            let mut buff = String::new();
            let p = read_json_from_file("config.json").unwrap();
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/tags").unwrap_or_else(|err| { error_reporter(err); });
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/commands").unwrap_or_else(|err| { error_reporter(err); });
            send_raw_message(&mut wstream, "CAP REQ :twitch.tv/membership").unwrap_or_else(|err| { error_reporter(err); });
            send_raw_message(&mut wstream, &format!("NICK justinfan{}", rng.gen_range(10000000..99999999))).unwrap_or_else(|err| { error_reporter(err); });
            send_raw_message(&mut wstream, &format!("JOIN #{}", channel)).unwrap_or_else(|err| { error_reporter(err); });

            while std::io::BufRead::read_line(&mut stream, &mut buff)? > 0 {
                let buffer = buff.trim();
                if buffer.contains(":tmi.twitch.tv CAP * ACK :twitch.tv/tags") {
                    println!("Tags request acknowledged")
                } else if buffer.contains(":tmi.twitch.tv CAP * ACK :twitch.tv/commands") {
                    println!("Commands request acknowledged")
                } else if buffer.contains(":tmi.twitch.tv CAP * ACK :twitch.tv/membership") {
                    println!("Membership request acknowledged")
                } else if buffer.contains(":Welcome, GLHF!") {
                    println!("\n[{}]\nConnected to Twitch IRC\n", Local::now().format("%T %d/%m/%G").to_string());
                }

                if buffer.contains("PRIVMSG") {
                    let channel: Vec<&str> = buffer.split(" ").collect();
                    let channel = channel[3];
                    let user = extract_tags(&buffer);
                    let raw_user: Vec<&str> = user["user-type"].split(|c| c == '!' || c == '@').collect();
                    let raw_user = raw_user[1];
                    let raw_message = buffer.rsplit(&format!("{} :", channel)).next().unwrap().trim();
                    let raw_user = raw_user.to_string();
                    let user_id = user["user-id"].to_string();
                    println!("[{}] [{}] {}[{}]: {}", channel, Local::now().format("%T %d/%m/%G").to_string(), raw_user, user_id, raw_message);

                    match Client::connect(&format!("postgresql://{}:{}@localhost:5432/{}", p.username, p.password, &CHANNEL), NoTls) {
                        Ok(mut conn) => {
                            match conn.query("select pid, state, usename, query, query_start from pg_stat_activity where pid in (select pid from pg_locks l join pg_class t on l.relation = t.oid and t.relkind = 'r' where t.relname = 'messages')", &[]) {
                                Ok(trans_pid) => {
                                    if trans_pid.is_empty() {
                                        let stmt = conn.prepare("INSERT INTO messages (date, username, user_id, message) VALUES ($1, $2, $3, $4)")?;
                                        if Path::new(&queued_messages_path).exists() {
                                            let queued_messages_file = File::open(&queued_messages_path)?;
                                            let read = BufReader::new(queued_messages_file);
                                            for i in read.lines() {
                                                let line = i.unwrap_or_default();
                                                let split: Vec<_> = line.split(" ").collect();
                                                println!("{}From queue: {}{}", color::Fg(color::Green), split.join(" "), color::Fg(color::Reset));
                                                let date = NaiveDate::parse_from_str(split[0], "%Y-%m-%d")?;
                                                let time = NaiveTime::parse_from_str(split[1], "%T")?;
                                                let nt: NaiveDateTime = NaiveDateTime::new(date, time); // 2021-06-25 00:00:00
                                                match conn.execute(&stmt, &[&nt, &split[2], &split[3], &split[4..].join(" ")]) {
                                                    Ok(_) => {}
                                                    Err(e) => {
                                                        println!("{:?}", e);
                                                        socket.shutdown(Both).unwrap_or_default();
                                                    }
                                                }
                                            }
                                            remove_file(&queued_messages_path).unwrap_or_else(|err| error_reporter(err));
                                        }
                                        let date = NaiveDate::parse_from_str(&Local::now().format("%Y-%m-%d").to_string(), "%Y-%m-%d")?;
                                        let time = NaiveTime::parse_from_str(&Local::now().format("%T").to_string(), "%H:%M:%S")?;
                                        let nt: NaiveDateTime = NaiveDateTime::new(date, time);
                                        match conn.execute(&stmt, &[&nt, &raw_user, &user_id, &raw_message]) {
                                            Ok(_) => {}
                                            Err(e) => {
                                                eprintln!("Error 1 writing to db: {:?}\nAdding message to queue and restarting...\n", e);
                                                let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                                                queue_message(message_to_queue).unwrap_or_else(|err| eprintln!("{}An error occurred while logging message to file: {}{}", color::Fg(color::Red), err, color::Fg(color::Reset)));
                                                socket.shutdown(Both).unwrap_or_default(); // We do this to break out of the loop imediately instead of on the next message which would cause us to lose a message
                                                break;
                                            }
                                        }
                                        conn.close()?;
                                    } else {
                                        println!("Messages table is busy adding message to queue...");
                                        let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                                        queue_message(message_to_queue).unwrap_or_else(|err| eprintln!("{}An error occurred while logging message to file: {}{}", color::Fg(color::Red), err, color::Fg(color::Reset)));
                                    }
                                }
                                Err(e) => {
                                    eprintln!("An error occured inside the db!: {:?}", e);
                                    let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                                    queue_message(message_to_queue).unwrap_or_else(|err| eprintln!("{}An error occurred while logging message to file: {:?}{}", color::Fg(color::Red), err, color::Fg(color::Reset)));

                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error 2 writing to db: {:?}\nAdding message to queue and restarting...", e);
                            let message_to_queue = format!("{} {} {} {}", Local::now().format("%G-%m-%d %T"), raw_user, user_id, raw_message);
                            queue_message(message_to_queue).unwrap_or_else(|err| eprintln!("{}An error occurred while logging message to file: {}{}", color::Fg(color::Red), err, color::Fg(color::Reset)));
                        }
                    }
                }

                if buffer.contains("PING :tmi.twitch.tv") {
                    send_raw_message(&mut wstream, "PONG :tmi.twitch.tv").unwrap_or_else(|err| { error_reporter(err); });
                    println!("[chat_logger.rs] PONG at {}", Local::now().format("%T %d/%m/%G").to_string());
                }
                buff.clear();
            }
            if std::io::BufRead::read_line(&mut stream, &mut buff)? == 0 {
                socket.shutdown(Shutdown::Write).unwrap_or_else(|err| eprintln!("Failed to shutdown socket: {}", err));
                eprintln!("\n{}Socket disconnected{}", color::Fg(color::Red), color::Fg(color::Reset));
            }
        }
        Err(e) => error_reporter(e),
    };

    fn send_raw_message<W: Write>(w: &mut W, msg: &str) -> Result<(), std::io::Error> {
        let message = format!("{}\r\n", msg);
        match w.write(message.as_bytes()) {
            Ok(_) => {
                match w.flush() {
                    Ok(_) => {}
                    Err(e) => eprintln!("Failed to send raw data: {:?}", e)
                }
            }
            Err(e) => eprintln!("Failed to write raw data into buffer: {:?}", e),
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
    let p = read_json_from_file("config.json")?;
    process::Command::new("sh").arg("scripts/create_db.sh").arg(&p.username).arg(CHANNEL.to_string()).spawn()?.wait()?;
    let mut conn = Client::connect(&format!("postgresql://{}:{}@localhost:5432/{}", p.username, p.password, &CHANNEL), NoTls).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS messages(
                    date TIMESTAMP WITHOUT TIME ZONE,
                    username VARCHAR(40),
                    user_id VARCHAR(30),
                    message VARCHAR(700) NOT NULL);", &[])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_messages_date_username ON public.messages USING btree (date, username)", &[])?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_messages_username_date ON public.messages USING btree (username, date)", &[])?;
    conn.close()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Any + Send + 'static>> {
    std::process::Command::new("clear").status().unwrap();
    let config = Config::new(env::args()).unwrap_or_else(|err| {
        eprintln!("{}Error: {}{}", color::Fg(color::Red), err, color::Fg(color::Reset));
        process::exit(1);
    });
    create_dir("channels").unwrap_or_else(|err| {
        if !err.to_string().to_lowercase().contains("file exists") {
            eprintln!("Error creating \"channels\": {:?}\nQuiting!", err);
            process::exit(1);
        }
    });

    unsafe {
        CHANNEL = config.channel;
        create_database().unwrap_or_else(|err| {
            println!("Failed to setup database for {}:\n{:?}", CHANNEL, err);
            process::exit(1);
        });
    }

    let builder = thread::Builder::new().name("Chat_logger".to_owned());
    builder.spawn(|| {
        loop {
            thread::spawn(|| unsafe {
                bot(CHANNEL.to_string()).unwrap_or_else(|err| {eprintln!("{}{:?}{}", color::Fg(color::Red), err, color::Fg(color::Reset))});
                println!("Chat_logger ending. Attempting to repeat loop...");
                sleep(100);
            }).join().unwrap_or_else(|err| println!("Inner Chat_logger thread crashed: {:?}", err));
            sleep(1000)
        }
    }).unwrap().join()?;
    Ok(())
}
