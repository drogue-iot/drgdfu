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
pub struct Report {
    version: String,
    mtu: Option<u32>,
    status: Option<ReportStatus>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReportStatus {
    version: String,
    offset: u32,
}

impl Report {
    pub fn first(version: &str) -> Self {
        Self {
            version: version.to_string(),
            mtu: None,
            status: None,
        }
    }

    pub fn status(current_version: &str, offset: u32, next_version: &str) -> Self {
        Self {
            version: current_version.to_string(),
            mtu: None,
            status: Some(ReportStatus {
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Operation {
    Sync,
    Write,
    Swap,
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
    pub async fn report(&self, report: &Report) -> Result<Command, FirmwareError> {
        match &self {
            Self::File { metadata, data } => {
                if metadata.version == report.version {
                    Ok(Command::new_sync(&report.version, None))
                } else {
                    if let Some(status) = &report.status {
                        if status.version == metadata.version {
                            if status.offset as usize == metadata.size {
                                Ok(Command::new_swap(&metadata.version, &[]))
                            } else {
                                let mtu = report.mtu.unwrap_or(4096) as usize;
                                let to_copy =
                                    core::cmp::min(mtu, data.len() - status.offset as usize);
                                let s =
                                    &data[status.offset as usize..status.offset as usize + to_copy];
                                Ok(Command::new_write(&metadata.version, status.offset, s))
                            }
                        } else {
                            log::info!("Updating with wrong status, starting over");
                            let mtu = report.mtu.unwrap_or(4096) as usize;
                            let to_copy = core::cmp::min(mtu, data.len());
                            Ok(Command::new_write(&metadata.version, 0, &data[..to_copy]))
                        }
                    } else {
                        let mtu = report.mtu.unwrap_or(4096) as usize;
                        let to_copy = core::cmp::min(mtu, data.len());
                        Ok(Command::new_write(&metadata.version, 0, &data[..to_copy]))
                    }
                }
            }
            _ => todo!(),
        }
    }
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

/*


impl FirmwareMetadata {
    pub fn from_file(path: &Path) -> Result<Self, FirmwareError> {
        let data = std::fs::read_to_string(path)?;
        let metadata = serde_json::from_str(&data)?;
        Ok(metadata)
    }

    pub fn from_http(url: String, size: usize, version: String) -> Self {
        Self {
            version,
            size,
            data: FirmwareData::Http(url),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FirmwareData {
    #[serde(rename = "file")]
    File(PathBuf),
    #[serde(rename = "http")]
    Http(String),
}

pub struct Deployment {
    pub id: String,
    pub metadata: FirmwareMetadata,
}

pub struct FirmwareClient {
    url: String,
}

impl FirmwareClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub async fn fetch_firmware(&self, url: &str) -> Result<Bytes, anyhow::Error> {
        let client = reqwest::Client::new();
        let res = client.get(url).send().await?.bytes().await?;
        Ok(res)
    }

    pub async fn wait_update(&self, current_version: &str) -> Result<Deployment, anyhow::Error> {
        let client = reqwest::Client::new();
        loop {
            let url = format!("{}/v1/poll/burrboard", self.url);
            let res = client.get(&url).send().await;

            let poll: Duration = match res {
                Ok(res) => {
                    let j: serde_json::Value = res.json().await.unwrap();
                    println!("RESULT:{:?}", j);

                    if let Some(current) = j.get("current") {
                        if let Some(version) = current["version"].as_str() {
                            if version != current_version {
                                let size = current["size"]
                                    .as_str()
                                    .ok_or(anyhow!("error reading firmware size"))?
                                    .parse::<usize>()?;
                                let path = format! {"{}/v1/fetch/burrboard/{}", self.url, version};
                                return Ok(Deployment {
                                    id: version.to_string(),
                                    metadata: FirmwareMetadata::from_http(
                                        path,
                                        size as usize,
                                        version.to_string(),
                                    ),
                                });
                            }
                        }
                    }

                    if let Some(interval) = j["interval"].as_i64() {
                        Duration::from_secs(interval as u64)
                    } else {
                        Duration::from_secs(5)
                    }
                }
                Err(e) => {
                    println!("ERROR FIRMWARE: {:?}", e);
                    Duration::from_secs(5)
                }
            };
            println!("Polling firmware server in {} seconds", poll.as_secs());
            tokio::time::sleep(poll).await;
        }
    }
}
*/
