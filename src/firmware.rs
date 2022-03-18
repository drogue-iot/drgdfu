use crate::FirmwareSource;
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use clap::{Parser, Subcommand};
use core::future::Future;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug)]
pub struct FirmwareFileMeta {
    pub version: String,
    pub size: usize,
    pub file: PathBuf,
}

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
                url,
                application,
                device,
                password,
            } => {
                todo!()
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Status {
    version: String,
    mtu: Option<u32>,
    update: Option<UpdateStatus>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateStatus {
    version: String,
    offset: u32,
}

impl Status {
    pub fn first(version: &str, mtu: Option<u32>) -> Self {
        Self {
            version: version.to_string(),
            mtu,
            update: None,
        }
    }

    pub fn update(version: &str, mtu: Option<u32>, offset: u32, next_version: &str) -> Self {
        Self {
            version: version.to_string(),
            mtu,
            update: Some(UpdateStatus {
                offset,
                version: next_version.to_string(),
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    Sync {
        version: String,
        poll: Option<u32>,
    },
    Write {
        version: String,
        offset: u32,
        data: Vec<u8>,
    },
    Swap {
        version: String,
        checksum: Vec<u8>,
    },
}

impl Command {
    pub fn new_sync(version: &str, poll: Option<u32>) -> Self {
        Self::Sync {
            version: version.to_string(),
            poll,
        }
    }

    pub fn new_swap(version: &str, checksum: &[u8]) -> Self {
        Self::Swap {
            version: version.to_string(),
            checksum: checksum.into(),
        }
    }

    pub fn new_write(version: &str, offset: u32, data: &[u8]) -> Self {
        Self::Write {
            version: version.to_string(),
            offset,
            data: data.into(),
        }
    }
}

impl FirmwareUpdater {
    async fn report(&self, status: &Status) -> Result<Command, FirmwareError> {
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
                Command::Sync { version, poll } => {
                    log::info!("Firmware in sync");
                    device.synced().await?;
                    return Ok(true);
                }
                Command::Swap { version, checksum } => {
                    log::info!("Swap operation");
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
            log::info!("Device reports version {}", current_version);
            if self.check(&current_version, device).await? {
                log::info!("Firmware updated");
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
        let mut f = File::open(path)?;
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
