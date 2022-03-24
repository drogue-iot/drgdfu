use crate::FirmwareSource;
use async_trait::async_trait;
use drogue_ajour_protocol::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

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
        application: String,
        device: String,
        password: String,
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
                url: _,
                application: _,
                device: _,
                password: _,
            } => {
                todo!()
            }
        }
    }
}

impl FirmwareUpdater {
    async fn report<'m>(&'m self, status: &Status<'m>) -> Result<Command<'m>, FirmwareError> {
        match &self {
            Self::File { metadata, data } => {
                if metadata.version == status.version {
                    Ok(Command::new_sync(&status.version, None))
                } else {
                    if let Some(update) = &status.update {
                        if update.version == metadata.version {
                            if update.offset as usize == metadata.size {
                                Ok(Command::new_swap(&metadata.version, &[]))
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
            _ => todo!(),
        }
    }

    async fn check<F: FirmwareDevice>(
        &self,
        current_version: &str,
        device: &mut F,
    ) -> Result<bool, anyhow::Error> {
        let mut status = Status::first(&current_version, Some(F::MTU));
        loop {
            let cmd = self.report(&status).await?;
            match cmd {
                Command::Write {
                    version,
                    offset,
                    data,
                } => {
                    if offset == 0 {
                        println!(
                            "Updating device firmware from {} to {}",
                            current_version, version
                        );
                        device.start().await?;
                    }
                    device.write(offset, &data).await?;
                    status = Status::update(
                        &current_version,
                        Some(F::MTU),
                        offset + data.len() as u32,
                        &version,
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
                    checksum: _,
                } => {
                    println!("Firmware written, instructing device to swap");
                    device.swap().await?;
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
    async fn swap(&mut self) -> Result<(), anyhow::Error>;
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
