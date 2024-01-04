use std::{
    io::{Read, Write},
    process::{self, Command, Stdio},
};

use anyhow::{bail, Context, Result};
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
    pub place_file: Option<String>,
    pub universe_id: Option<u64>,
    pub place_id: Option<u64>,
    pub creator_id: Option<u64>,
    pub creator_type: Option<u8>,
    pub no_launch: bool,
    pub oneshot: bool,
    pub team_test: bool,
}

impl PlaceRunner {
    pub fn install_plugin() -> Result<()> {
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

        if std::fs::create_dir_all(studio_install.plugins_path()).is_err() {
            bail!("could not create plugins directory - are you missing permissions to write to `{:?}`?", studio_install.plugins_path());
        }

        let plugin_file_path = studio_install.plugins_path().join("run_in_roblox.rbxm");

        let mut plugin_file = File::create(plugin_file_path)?;
        plugin_file.write_all(&local_plugin_data)?;
        Ok(())
    }

    pub fn remove_plugin() -> Result<()> {
        let studio_install =
            RobloxStudio::locate().context("Could not locate a Roblox Studio installation.")?;

        let plugin_file_path = studio_install.plugins_path().join("run_in_roblox.rbxm");

        std::fs::remove_file(plugin_file_path)?;
        Ok(())
    }

    pub async fn run(
        &self,
        sender: async_channel::Sender<Option<RobloxMessage>>,
        exit_receiver: async_channel::Receiver<()>,
    ) -> Result<(), anyhow::Error> {
        let studio_install =
            RobloxStudio::locate().context("Could not locate a Roblox Studio installation.")?;

        Self::install_plugin()?;

        let studio_args = match &self.team_test {
            true => {
                vec![
                    "-task".to_string(),
                    "StartTeamTest".to_string(),
                    "-placeId".to_string(),
                    format!("{:}", self.place_id.unwrap()),
                    "-universeId".to_string(),
                    format!("{:}", self.universe_id.unwrap()),
                ]
            }
            false => {
                let place_file = self.place_file.as_ref().unwrap();
                std::fs::copy(
                    place_file,
                    dbg!(studio_install.plugins_path().join("../server.rbxl")),
                )?;
                vec![
                    "-task".to_string(),
                    "StartServer".to_string(),
                    "-placeId".to_string(),
                    format!("{:}", self.place_id.unwrap()),
                    "-universeId".to_string(),
                    format!("{:}", self.universe_id.unwrap()),
                    "-creatorId".to_string(),
                    format!("{:}", self.creator_id.unwrap()),
                    "-creatorType".to_string(),
                    format!("{:}", self.creator_type.unwrap()),
                    "-numtestserverplayersuponstartup".to_string(),
                    "0".to_string(),
                ]
            }
        };

        let api_svc = message_receiver::Svc::start()
            .await
            .expect("api service to start");

        let studio_process = if self.no_launch {
            None
        } else {
            info!("launching roblox studio with args {studio_args:?}");
            Some(KillOnDrop(
                Command::new(studio_install.application_path())
                    .args(studio_args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?,
            ))
        };

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
                                        oneshot: self.oneshot
                                    },
                                )
                                .await;
                        }
                        Message::Stop { server } => {
                            info!("studio server {server:} stopped");
                            if self.oneshot {
                                info!("now exiting as --oneshot was set to true.");
                                break;
                            }
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
        Self::remove_plugin()?;

        Ok(())
    }
}
