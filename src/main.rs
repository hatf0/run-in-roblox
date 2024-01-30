#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::struct_excessive_bools)]
mod message_receiver;
mod place_runner;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use fs_err::File;
use log::{error, info, warn};
use std::io::Write;
use std::process;
use std::{io::Read, sync::Arc};
use tokio::{signal, sync::Mutex};

use crate::{
    message_receiver::{OutputLevel, RobloxMessage},
    place_runner::PlaceRunner,
};

#[derive(Debug, Parser)]
enum Cli {
    Run(RunOptions),
}

#[derive(Debug, clap::Args)]
#[command(author, version, about, long_about = None)]
struct RunOptions {
    /// The script file to run
    #[arg(short, long, required(true))]
    script: String,

    /// The path to a place file to run (unless using team test)
    #[arg(long, required_unless_present("team_test"))]
    place_file: Option<String>,

    /// The universe ID of the place file
    #[arg(long, required(true))]
    universe_id: Option<u64>,

    /// The place ID of the place file
    #[arg(long, required(true))]
    place_id: Option<u64>,

    /// The creator ID of the universe / place
    #[arg(long, required_unless_present("team_test"))]
    creator_id: Option<u64>,

    /// The creator type of the universe / place (usually 0 for an individual, 1 for a group)
    #[arg(long, required_unless_present("team_test"), default_value("0"))]
    creator_type: Option<u8>,

    /// The number of client instances to launch while opening this place file. You can also run scripts on these clients.
    #[arg(long, default_value("0"))]
    num_clients: u8,

    /// Should this program exit after the first instance disconnects / times out?
    #[arg(short, long)]
    oneshot: bool,

    /// Use this flag if the lifecycle of Roblox Studio is managed by you. Note that you will need to restart Roblox Studio for the plugin to be installed.
    #[arg(long)]
    no_launch: bool,

    /// Use this flag if you want to keep the Roblox Studio instance around after this program exits. This is typically used with --no_launch to do repeat testing.
    #[arg(long)]
    no_exit: bool,

    /// Use this flag if you want to open an existing place published by a group. This is still experimental and has not been tested.
    #[arg(short, long)]
    team_test: bool,
}

async fn run(options: RunOptions) -> Result<i32> {
    let mut script = File::open(options.script)?;
    let mut str = String::default();
    script.read_to_string(&mut str)?;

    let place_runner = PlaceRunner {
        port: 7777,
        script: str,
        universe_id: options.universe_id,
        place_file: options.place_file,
        place_id: options.place_id,
        oneshot: options.oneshot,
        no_launch: options.no_launch,
        team_test: options.team_test,
        creator_id: options.creator_id,
        creator_type: options.creator_type,
        no_exit: options.no_exit,
        num_clients: options.num_clients,
    };

    let (exit_sender, exit_receiver) = async_channel::unbounded::<()>();
    let (sender, receiver) = async_channel::unbounded::<Option<RobloxMessage>>();

    let exit_receiver_clone = exit_receiver.clone();
    let place_runner_task = tokio::task::spawn(async move {
        tokio::select! {
            r = place_runner.run(sender, exit_receiver_clone) => {
                match r {
                    Ok(()) => Ok(()),
                    Err(e) => Err(e)
                }
            },
        }
    });

    let exit_code: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));

    let exit_code_clone = exit_code.clone();
    let exit_receiver_clone = exit_receiver.clone();
    let printer_task = tokio::task::spawn(async move {
        loop {
            tokio::select! {
                Ok(message) = receiver.recv() => {
                    match message {
                        Some(RobloxMessage::Output { level, body, server }) => {
                            let server = format!("studio-{}", &server[0..7]);

                            match level {
                                OutputLevel::Print => info!(target: &server, "{body:}"),
                                OutputLevel::Info => info!(target: &server, "{body:}"),
                                OutputLevel::Warning => warn!(target: &server, "{body:}"),
                                OutputLevel::Error => error!(target: &server, "{body:}"),
                                OutputLevel::ScriptError => error!(target: &server, "{body:}"),
                            };

                            if level == OutputLevel::ScriptError {
                                warn!("exiting with code 1 due to script erroring");
                                let mut exit_code = exit_code_clone.lock().await;
                                *exit_code = 1;
                            }
                        }
                        None => return,
                    }
                },
                _ = exit_receiver_clone.recv() => return
            }
        }
    });

    async fn close_shop(exit_sender: &async_channel::Sender<()>) {
        exit_sender.send(()).await.unwrap();
    }

    tokio::select! {
        res = place_runner_task => {
            let exit_code = match res.unwrap() {
                Ok(()) => {
                    let exit_code = exit_code.lock().await;
                    *exit_code
                },
                Err(e) => {
                    error!("place runner task exited early with err: {e:?}");
                    1
                }
            };
            close_shop(&exit_sender).await;
            Ok(exit_code)
        }
        _ = printer_task => {
            warn!("printer task exited early - closing up shop");
            close_shop(&exit_sender).await;
            Ok(1)
        },
        _ = signal::ctrl_c() => {
            info!("goodbye!");
            close_shop(&exit_sender).await;
            let exit_code = exit_code.lock().await;
            Ok(*exit_code)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = Cli::parse();
    let log_env = env_logger::Env::default().default_filter_or("info");

    env_logger::Builder::from_env(log_env)
        .format(|buf, record| {
            let level = match record.level() {
                log::Level::Debug => "DEBUG".dimmed(),
                log::Level::Trace => "TRACE".white(),
                log::Level::Info => "INFO".green(),
                log::Level::Warn => "WARN".yellow().bold(),
                log::Level::Error => "ERROR".red().bold(),
            };
            let ts = buf.timestamp_seconds();
            let args = record.args().to_string();
            let args = match record.level() {
                log::Level::Debug => args.dimmed(),
                log::Level::Trace => args.white(),
                log::Level::Info => args.green(),
                log::Level::Warn => args.yellow().bold(),
                log::Level::Error => args.red().bold(),
            };
            writeln!(buf, "[{} {} {}] {}", ts, level, record.target(), args)
        })
        .init();

    match options {
        Cli::Run(options) => match run(options).await {
            Ok(exit_code) => process::exit(exit_code),
            Err(err) => {
                log::error!("{:?}", err);
                process::exit(2);
            }
        },
    }
}
