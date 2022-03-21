use crate::firmware::FirmwareDevice;
use async_trait::async_trait;
use bluer::{
    gatt::remote::{Characteristic, Service},
    Adapter, AdapterEvent, Address, Device,
};
use futures::pin_mut;
use futures::StreamExt;
use std::str::FromStr;
use tokio::time::{sleep, Duration};

pub struct GattBoard {
    adapter: Adapter,
    device: String,
    board: Option<Device>,
    updated: bool,
}

const FIRMWARE_SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000186100001000800000805f9b34fb);
const CONTROL_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123600001000800000805f9b34fb);
const OFFSET_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123500001000800000805f9b34fb);
const FIRMWARE_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123400001000800000805f9b34fb);
const VERSION_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x0000123700001000800000805f9b34fb);

impl GattBoard {
    pub fn new(device: &str, adapter: Adapter) -> Self {
        Self {
            device: device.to_string(),
            adapter,
            board: None,
            updated: false,
        }
    }

    async fn read_firmware_offset(&mut self) -> bluer::Result<u32> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, OFFSET_CHAR_UUID)
            .await?;
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    async fn read_firmware_version(&mut self) -> bluer::Result<String> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, VERSION_CHAR_UUID)
            .await?;
        Ok(String::from_str(core::str::from_utf8(&data).unwrap()).unwrap())
    }

    async fn mark_booted(&mut self) -> bluer::Result<()> {
        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[4])
            .await
    }

    async fn start_firmware_update(&mut self) -> Result<(), anyhow::Error> {
        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[1])
            .await?;

        println!("Triggered DFU init sequence");
        // Wait until firmware offset is reset
        while self.read_firmware_offset().await? != 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Ok(())
    }

    async fn write_firmware(
        &mut self,
        mut offset: u32,
        firmware: &[u8],
    ) -> Result<(), anyhow::Error> {
        const CHUNK_SIZE: usize = 64;
        let mut buf = [0; CHUNK_SIZE];
        for chunk in firmware.chunks(CHUNK_SIZE) {
            buf[0..chunk.len()].copy_from_slice(chunk);
            if chunk.len() < buf.len() {
                buf[chunk.len()..].fill(0);
            }
            self.write_char(FIRMWARE_SERVICE_UUID, FIRMWARE_CHAR_UUID, &buf)
                .await?;
            log::info!("Write {} bytes at offset {}", buf.len(), offset);
            offset += buf.len() as u32;
            if offset % 4096 == 0 {
                println!("{} bytes written", offset)
            }

            // Wait until firmware offset is incremented
            while self.read_firmware_offset().await? != offset {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
        Ok(())
    }

    async fn swap_firmware(&mut self) -> Result<(), anyhow::Error> {
        // Write signal that DFU process is done and should be applied
        log::info!("DFU process done, setting reset");
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[2])
            .await?;

        Ok(())
    }

    async fn read_char(&mut self, service: uuid::Uuid, c: uuid::Uuid) -> bluer::Result<Vec<u8>> {
        let service = self.find_service(service).await?.unwrap();
        let c = self.find_char(&service, c).await?.unwrap();

        let value = c.read().await?;
        Ok(value)
    }

    async fn write_char(
        &mut self,
        service: uuid::Uuid,
        c: uuid::Uuid,
        value: &[u8],
    ) -> bluer::Result<()> {
        let service = self.find_service(service).await?.unwrap();
        let c = self.find_char(&service, c).await?.unwrap();

        c.write(value).await
    }

    async fn find_char(
        &mut self,
        service: &Service,
        characteristic: uuid::Uuid,
    ) -> bluer::Result<Option<Characteristic>> {
        for c in service.characteristics().await? {
            let uuid = c.uuid().await?;
            if uuid == characteristic {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }

    async fn find_service(&mut self, service: uuid::Uuid) -> bluer::Result<Option<Service>> {
        let device = self.connect().await?;
        for s in device.services().await? {
            let uuid = s.uuid().await?;
            if uuid == service {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }

    async fn connect(&mut self) -> bluer::Result<&mut Device> {
        if self.board.is_none() {
            let session = bluer::Session::new().await?;
            let adapter = session.default_adapter().await?;
            adapter.set_powered(true).await?;
            let discover = adapter.discover_devices().await?;
            pin_mut!(discover);

            let addr = Address::from_str(&self.device)?;

            while let Some(evt) = discover.next().await {
                match evt {
                    AdapterEvent::DeviceAdded(a) if a == addr => {
                        let device = adapter.device(a)?;

                        sleep(Duration::from_secs(2)).await;
                        if !device.is_connected().await? {
                            log::debug!("Connecting...");
                            let mut retries = 2;
                            loop {
                                match device.connect().await {
                                    Ok(()) => break,
                                    Err(err) if retries > 0 => {
                                        println!("Connect error: {}", &err);
                                        retries -= 1;
                                    }
                                    Err(err) => return Err(err.into()),
                                }
                            }
                            log::debug!("Connected");
                        } else {
                            log::debug!("Already connected");
                        }
                        self.board.replace(device);
                        break;
                    }
                    AdapterEvent::DeviceRemoved(a) if a == addr => {
                        log::info!("Device removed: {}", a);
                    }
                    _ => {}
                }
            }
        }
        Ok(self.board.as_mut().unwrap())
    }
}

#[async_trait]
impl FirmwareDevice for GattBoard {
    const MTU: u32 = 128;
    async fn version(&mut self) -> Result<String, anyhow::Error> {
        log::info!("Reading version");
        Ok(self.read_firmware_version().await?)
    }

    async fn start(&mut self) -> Result<(), anyhow::Error> {
        log::info!("Start updated");
        Ok(self.start_firmware_update().await?)
    }
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error> {
        Ok(self.write_firmware(offset, data).await?)
    }
    async fn swap(&mut self) -> Result<(), anyhow::Error> {
        log::info!("Swapping firmware");
        let r = Ok(self.swap_firmware().await?);
        self.adapter
            .remove_device(self.board.take().unwrap().address())
            .await?;
        self.updated = true;
        r
    }

    async fn synced(&mut self) -> Result<(), anyhow::Error> {
        if self.updated {
            log::info!("Mark as booted");
            Ok(self.mark_booted().await?)
        } else {
            log::info!("Not updated?!");
            Ok(())
        }
    }
}
