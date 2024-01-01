mod message_receiver;
mod place_runner;

use std::process;
use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use fs_err::File;
use std::io::Read;  
use tokio::signal;

use crate::{
    message_receiver::{OutputLevel, RobloxMessage},
    place_runner::PlaceRunner,
};

#[derive(Debug, Parser)]
enum Cli {
    Run(CliOptions),
}

#[derive(Debug, clap::Args)]
#[command(author, version, about, long_about = None)]
struct CliOptions {
    script: String
}

async fn run(options: Cli) -> Result<i32> {
    let Cli::Run(CliOptions { script: script_src }) = options else {
        unreachable!()
    };
    let mut script = File::open(script_src)?;
    let mut str = String::default();
    script.read_to_string(&mut str)?;

    let place_runner = PlaceRunner { port: 7777, script: str };

    let (sender, receiver) = async_channel::unbounded::<Option<RobloxMessage>>();

    let place_runner_task = tokio::task::spawn(async move {
        match place_runner.run(sender).await {
            Ok(_) => (),
            Err(e) => println!("place runner exited with: {e:?}"),
        }
    });

    let mut exit_code = 0;

    let printer_task = tokio::task::spawn(async move {
        while let Ok(message) = receiver.recv().await {
            match message {
                Some(RobloxMessage::Output { level, body }) => {
                    let colored_body = match level {
                        OutputLevel::Print => body.normal(),
                        OutputLevel::Info => body.cyan(),
                        OutputLevel::Warning => body.yellow(),
                        OutputLevel::Error => body.red(),
                    };

                    println!("{}", colored_body);

                    if level == OutputLevel::Error {
                        exit_code = 1;
                    }
                }
                None => (),
            }
        }
    });

    tokio::select! {
        _ = signal::ctrl_c() => {
            println!("goodbye!");
            place_runner_task.abort();
            printer_task.abort();
        }
    }

    Ok(exit_code)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = Cli::parse();
    let log_env = env_logger::Env::default().default_filter_or("warn");

    env_logger::Builder::from_env(log_env)
        .format_timestamp(None)
        .init();

    match run(options).await {
        Ok(exit_code) => process::exit(exit_code),
        Err(err) => {
            log::error!("{:?}", err);
            process::exit(2);
        }
    }
}
