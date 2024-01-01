use std::{process::{self, Command, Stdio}, io::{Read, Write}};

use anyhow::{anyhow, bail, Context};
use fs_err::File;
use roblox_install::RobloxStudio;

use crate::message_receiver::{self, Message, RobloxMessage};

/// A wrapper for process::Child that force-kills the process on drop.
struct KillOnDrop(process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ignored = self.0.kill();
    }
}

pub struct PlaceRunner {
    pub port: u16,
    pub script: String,
}

impl PlaceRunner {
    pub async fn run(
        &self,
        sender: async_channel::Sender<Option<RobloxMessage>>,
    ) -> Result<(), anyhow::Error> {
        let studio_install =
            RobloxStudio::locate().context("Could not locate a Roblox Studio installation.")?;

        let mut local_plugin = match File::open("./plugin/plugin.rbxm") {
            Err(_) => {
                bail!("could not open plugin file - did you build it with `lune`?");
            },
            Ok(file) => file
        };
        let mut local_plugin_data = vec![];
        local_plugin.read_to_end(&mut local_plugin_data)?;

        match std::fs::create_dir_all(studio_install.plugins_path()) {
            Err(_) => {
                bail!("could not create plugins directory - are you missing permissions to write to `{:?}`?", studio_install.plugins_path());
            }
            Ok(_) => ()
        }
        
        let plugin_file_path = studio_install
            .plugins_path()
            .join("run_in_roblox.rbxm");

        let mut plugin_file = File::create(&plugin_file_path)?;
        plugin_file.write_all(&local_plugin_data)?;

        let api_svc = message_receiver::Svc::start(self.script.clone())
            .await
            .expect("api service to start");

        let _studio_process = KillOnDrop(
            Command::new(studio_install.application_path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );

        loop {
            match api_svc.recv().await {
                Message::Start { server } => {
                    println!("server {server:?} started");
                }
                Message::Stop { server } => {
                    println!("server {server:?} stopped");
                }
                Message::Messages(roblox_messages) => {
                    for message in roblox_messages.into_iter() {
                        sender.send(Some(message)).await?;
                    }
                }
            }
        }
        api_svc.stop().await;
        sender.close();
        // fs::remove_file(&plugin_file_path)?;

        Ok(())
    }
}
