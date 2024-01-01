use std::{
    io::{Read, Write},
    process::{self, Command, Stdio},
};

use anyhow::{bail, Context};
use fs_err::File;
use log::info;
use roblox_install::RobloxStudio;

use crate::message_receiver::{self, Message, RobloxMessage};

/// A wrapper for `process::Child` that force-kills the process on drop.
struct KillOnDrop(process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ignored = self.0.kill();
    }
}

pub struct PlaceRunner {
    pub port: u16,
    pub script: String,
    pub universe_id: Option<u64>,
    pub place_id: Option<u64>,
    pub place_file: Option<String>,
}

impl PlaceRunner {
    pub async fn run(
        &self,
        sender: async_channel::Sender<Option<RobloxMessage>>,
        exit_receiver: async_channel::Receiver<()>,
    ) -> Result<(), anyhow::Error> {
        let studio_install =
            RobloxStudio::locate().context("Could not locate a Roblox Studio installation.")?;

        let mut local_plugin = match File::open("./plugin/plugin.rbxm") {
            Err(_) => {
                bail!("could not open plugin file - did you build it with `lune`?");
            }
            Ok(file) => file,
        };
        let mut local_plugin_data = vec![];
        local_plugin.read_to_end(&mut local_plugin_data)?;

        let studio_args = match (&self.place_file, self.universe_id, self.place_id) {
            (None, Some(universe_id), Some(place_id)) => {
                vec![
                    "-task".to_string(),
                    "EditPlace".to_string(),
                    "-placeId".to_string(),
                    format!("{place_id:}"),
                    "-universeId".to_string(),
                    format!("{universe_id:}")
                ]
            },
            (None, None, Some(place_id)) => {
                vec![
                    "-task".to_string(),
                    "EditPlace".to_string(),
                    "-placeId".to_string(),
                    format!("{place_id:}")
                ]
            },
            (Some(place_file), None, None) => {
                vec![
                    place_file.clone()
                ]
            },
            _ => bail!("invalid arguments passed - you may only specify a place_id (and optionally a universe_id), or a place_file, but not both")
        };

        if std::fs::create_dir_all(studio_install.plugins_path()).is_err() {
            bail!("could not create plugins directory - are you missing permissions to write to `{:?}`?", studio_install.plugins_path());
        }

        let plugin_file_path = studio_install.plugins_path().join("run_in_roblox.rbxm");

        let mut plugin_file = File::create(&plugin_file_path)?;
        plugin_file.write_all(&local_plugin_data)?;

        let api_svc = message_receiver::Svc::start()
            .await
            .expect("api service to start");

        info!("launching roblox studio with args {studio_args:?}");

        let studio_process = KillOnDrop(
            Command::new(studio_install.application_path())
                .args(studio_args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );

        loop {
            tokio::select! {
                msg = api_svc.recv() => {
                    match msg {
                        Message::Start { server } => {
                            info!("studio server {server:} started");
                            api_svc
                                .queue_event(
                                    server,
                                    message_receiver::RobloxEvent::RunScript {
                                        script: self.script.clone(),
                                    },
                                )
                                .await;
                        }
                        Message::Stop { server } => {
                            info!("studio server {server:} stopped");
                        }
                        Message::Messages(roblox_messages) => {
                            for message in roblox_messages {
                                sender.send(Some(message)).await?;
                            }
                        }
                    }
                }
                _ = exit_receiver.recv() => {
                    break;
                }
            }
        }
        // explicitly drop the studio process so it dies
        drop(studio_process);
        api_svc.stop().await;
        sender.close();
        std::fs::remove_file(&plugin_file_path)?;

        Ok(())
    }
}
