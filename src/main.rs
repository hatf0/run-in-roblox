#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::items_after_statements)]
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
    #[arg(short, long)]
    script: String,

    #[arg(long)]
    place_file: Option<String>,

    #[arg(long)]
    universe_id: Option<u64>,

    #[arg(long)]
    place_id: Option<u64>,

    #[arg(long)]
    creator_id: Option<u64>,

    #[arg(long)]
    creator_type: Option<u8>,

    #[arg(short, long)]
    oneshot: bool,

    #[arg(long)]
    no_launch: bool,

    #[arg(long)]
    no_exit: bool,

    #[arg(short, long)]
    team_test: bool,
}

async fn run(options: RunOptions) -> Result<i32> {
    let RunOptions {
        script,
        universe_id,
        place_id,
        place_file,
        oneshot,
        no_launch,
        team_test,
        creator_id,
        creator_type,
        no_exit
    } = options;
    let mut script = File::open(script)?;
    let mut str = String::default();
    script.read_to_string(&mut str)?;

    let place_runner = PlaceRunner {
        port: 7777,
        script: str,
        universe_id,
        place_file,
        place_id,
        oneshot,
        no_launch,
        team_test,
        creator_id,
        creator_type,
        no_exit
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
