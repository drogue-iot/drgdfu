use crate::FirmwareSource;
use anyhow::anyhow;
use bytes::Bytes;
use clap::{Parser, Subcommand};
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

pub enum FirmwareProducer {
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

impl FirmwareProducer {
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

impl FirmwareProducer {
    pub async fn report(&self, status: &Status) -> Result<Command, FirmwareError> {
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
}

// A device capable of updating it's firmware
#[async_trait]
pub trait FirmwareDevice {
    async fn start(&mut self) -> Result<(), anyhow::Error>;
}

#[derive(Debug)]
pub enum FirmwareError {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl FirmwareFileMeta {
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
