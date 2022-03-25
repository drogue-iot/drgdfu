use crate::FirmwareSource;
use anyhow::anyhow;
use async_trait::async_trait;
use drogue_ajour_protocol::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use tokio::time::{sleep, Duration};

#[derive(Serialize, Deserialize, Debug)]
pub struct FirmwareFileMeta {
    pub version: String,
    pub size: usize,
    pub file: PathBuf,
}

#[allow(dead_code)]
pub enum FirmwareUpdater {
    File {
        metadata: FirmwareFileMeta,
        data: Vec<u8>,
    },
    Cloud {
        url: String,
        user: String,
        password: String,
        client: reqwest::Client,
    },
}

impl FirmwareUpdater {
    pub fn new(source: &FirmwareSource) -> Result<Self, anyhow::Error> {
        match source {
            FirmwareSource::File { file } => {
                let metadata = FirmwareFileMeta::from_file(&file)?;
                let mut file = File::open(&metadata.file)?;
                let mut data = Vec::new();
                file.read_to_end(&mut data)?;
                Ok(Self::File { metadata, data })
            }
            FirmwareSource::Cloud {
                http,
                application,
                device,
                password,
            } => Ok(Self::Cloud {
                client: reqwest::Client::new(),
                url: format!("{}/v1/dfu", http),
                user: format!("{}@{}", device, application),
                password: password.to_string(),
            }),
        }
    }
}

impl FirmwareUpdater {
    async fn report<'m>(&'m self, status: &StatusRef<'m>) -> Result<Command, anyhow::Error> {
        match &self {
            Self::File { metadata, data } => {
                if metadata.version == status.version {
                    Ok(Command::new_sync(&status.version, None))
                } else {
                    if let Some(update) = &status.update {
                        if update.version == metadata.version {
                            if update.offset as usize == metadata.size {
                                Ok(Command::new_swap(&metadata.version, &[0; 32]))
                            } else {
                                let mtu = status.mtu.unwrap_or(4096) as usize;
                                let to_copy =
                                    core::cmp::min(mtu, data.len() - update.offset as usize);
                                let s =
                                    &data[update.offset as usize..update.offset as usize + to_copy];
                                Ok(Command::new_write(&metadata.version, update.offset, s))
                            }
                        } else {
                            log::info!("Updating with wrong status, starting over");
                            let mtu = status.mtu.unwrap_or(4096) as usize;
                            let to_copy = core::cmp::min(mtu, data.len());
                            Ok(Command::new_write(&metadata.version, 0, &data[..to_copy]))
                        }
                    } else {
                        let mtu = status.mtu.unwrap_or(4096) as usize;
                        let to_copy = core::cmp::min(mtu, data.len());
                        Ok(Command::new_write(&metadata.version, 0, &data[..to_copy]))
                    }
                }
            }
            Self::Cloud {
                url,
                user,
                client,
                password,
            } => loop {
                let payload = serde_json::to_string(status)?;
                let result = client
                    .post(url.clone())
                    .basic_auth(user, Some(password))
                    .query(&[("ct", 30)])
                    .json(&payload)
                    .send()
                    .await;

                match result {
                    Ok(r) if !r.status().is_success() => {
                        return Err(anyhow!(
                            "Error reporting status to cloud: {}: {}",
                            r.status(),
                            r.text().await.unwrap_or_default()
                        ))
                    }
                    Ok(r) => {
                        if let Ok(payload) = r.text().await {
                            log::trace!("Received command: {:?}", payload);
                            if let Ok(cmd) = serde_json::from_str::<Command>(&payload) {
                                return Ok(cmd);
                            } else {
                                log::trace!("Error parsing command, retrying in 1 sec");
                            }
                        }
                        sleep(Duration::from_secs(1)).await;
                    }
                    Err(e) => return Err(e.into()),
                }
            },
        }
    }

    async fn check<F: FirmwareDevice>(
        &self,
        current_version: &str,
        device: &mut F,
    ) -> Result<bool, anyhow::Error> {
        let mut status = StatusRef::first(&current_version, Some(F::MTU));
        #[allow(unused_mut)]
        #[allow(unused_assignments)]
        let mut v = String::new();
        loop {
            let cmd = self.report(&status).await?;
            match cmd {
                Command::Write {
                    version,
                    offset,
                    data,
                } => {
                    v = version.clone();
                    if offset == 0 {
                        println!(
                            "Updating device firmware from {} to {}",
                            current_version, version
                        );
                        device.start().await?;
                    }
                    device.write(offset, &data).await?;
                    status = StatusRef::update(
                        &current_version,
                        Some(F::MTU),
                        offset + data.len() as u32,
                        &v,
                    );
                }
                Command::Sync {
                    version: _,
                    poll: _,
                } => {
                    log::info!("Firmware in sync");
                    device.synced().await?;
                    return Ok(true);
                }
                Command::Swap {
                    version: _,
                    checksum,
                } => {
                    println!("Firmware written, instructing device to swap");
                    device.swap(checksum).await?;
                    return Ok(false);
                }
            }
        }
    }

    /// Run the firmware update protocol. Returns when firmware is fully in sync
    pub async fn run<F: FirmwareDevice>(&self, device: &mut F) -> Result<(), anyhow::Error> {
        loop {
            let current_version = device.version().await?;
            println!("Device reports version {}", current_version);
            if self.check(&current_version, device).await? {
                println!("Device is up to date");
                break;
            }
        }
        Ok(())
    }
}

// A device capable of updating it's firmware
#[async_trait]
pub trait FirmwareDevice {
    const MTU: u32;
    async fn version(&mut self) -> Result<String, anyhow::Error>;
    async fn start(&mut self) -> Result<(), anyhow::Error>;
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error>;
    async fn swap(&mut self, checksum: [u8; 32]) -> Result<(), anyhow::Error>;
    async fn synced(&mut self) -> Result<(), anyhow::Error>;
}

#[derive(Debug)]
pub enum FirmwareError {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl FirmwareFileMeta {
    pub fn new(version: &str, path: &PathBuf) -> Result<Self, FirmwareError> {
        let f = File::open(path)?;
        let metadata = f.metadata()?;
        let len = metadata.len();
        Ok(Self {
            version: version.to_string(),
            size: len as usize,
            file: path.clone(),
        })
    }
    pub fn from_file(path: &PathBuf) -> Result<Self, FirmwareError> {
        let data = std::fs::read_to_string(path)?;
        let metadata = serde_json::from_str(&data)?;
        Ok(metadata)
    }
}

impl core::fmt::Display for FirmwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), core::fmt::Error> {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Parse(e) => e.fmt(f),
        }
    }
}

impl From<std::io::Error> for FirmwareError {
    fn from(error: std::io::Error) -> Self {
        FirmwareError::Io(error)
    }
}

impl From<serde_json::Error> for FirmwareError {
    fn from(error: serde_json::Error) -> Self {
        FirmwareError::Parse(error)
    }
}

impl serde::ser::StdError for FirmwareError {}
