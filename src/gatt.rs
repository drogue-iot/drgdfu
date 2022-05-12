use crate::firmware::FirmwareDevice;
use async_trait::async_trait;
use bluer::{
    gatt::remote::{Characteristic, Service},
    Adapter, Address, Device,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

pub struct GattBoard {
    adapter: Arc<Adapter>,
    device: Address,
    board: Option<Device>,
    updated: bool,
    mtu: Option<u8>,
}

const FIRMWARE_SERVICE_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001000b0cd11ec871fd45ddf138840);

const VERSION_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001001b0cd11ec871fd45ddf138840);
const MTU_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001002b0cd11ec871fd45ddf138840);
const CONTROL_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001003b0cd11ec871fd45ddf138840);
const NEXT_VERSION_CHAR_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x00001004b0cd11ec871fd45ddf138840);
const OFFSET_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001005b0cd11ec871fd45ddf138840);
const FIRMWARE_CHAR_UUID: uuid::Uuid = uuid::Uuid::from_u128(0x00001006b0cd11ec871fd45ddf138840);

impl GattBoard {
    pub fn new(device: &str, adapter: Arc<Adapter>) -> Self {
        Self {
            device: Address::from_str(device).unwrap(),
            adapter,
            board: None,
            updated: false,
            mtu: None,
        }
    }

    pub fn address(&self) -> &Address {
        &self.device
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

    async fn read_mtu(&mut self) -> bluer::Result<u8> {
        let data = self.read_char(FIRMWARE_SERVICE_UUID, MTU_CHAR_UUID).await?;
        Ok(data[0])
    }

    async fn read_next_firmware_version(&mut self) -> bluer::Result<String> {
        let data = self
            .read_char(FIRMWARE_SERVICE_UUID, NEXT_VERSION_CHAR_UUID)
            .await?;
        Ok(String::from_str(core::str::from_utf8(&data).unwrap()).unwrap())
    }

    async fn mark_booted(&mut self) -> bluer::Result<()> {
        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[3])
            .await
    }

    async fn start_firmware_update(&mut self, version: &str) -> Result<(), anyhow::Error> {
        // Write the version we're updating
        self.write_char(
            FIRMWARE_SERVICE_UUID,
            NEXT_VERSION_CHAR_UUID,
            version.as_bytes(),
        )
        .await?;

        // Trigger DFU process
        self.write_char(FIRMWARE_SERVICE_UUID, CONTROL_CHAR_UUID, &[1])
            .await?;

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
        // Retrieve desired MTU size
        if self.mtu.is_none() {
            let mtu = self.read_mtu().await?;
            self.mtu.replace(mtu);
        }

        let mtu = self.mtu.unwrap() as usize;
        let mut buf = [0; u8::MAX as usize];
        for chunk in firmware.chunks(mtu) {
            buf[0..chunk.len()].copy_from_slice(chunk);
            if chunk.len() < mtu {
                buf[chunk.len()..mtu].fill(0);
            }
            self.write_char(FIRMWARE_SERVICE_UUID, FIRMWARE_CHAR_UUID, &buf[0..mtu])
                .await?;
            log::debug!("Write {} bytes at offset {}", mtu, offset);
            offset += mtu as u32;
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
            loop {
                if let Ok(device) = self.adapter.device(self.device) {
                    // Make sure we get a fresh start
                    let _ = device.disconnect().await;
                    sleep(Duration::from_secs(2)).await;
                    match device.is_connected().await {
                        Ok(false) => {
                            log::debug!("Connecting...");
                            loop {
                                match device.connect().await {
                                    Ok(()) => break,
                                    Err(err) => {
                                        log::error!("Connect error: {}", &err);
                                    }
                                }
                            }
                            log::debug!("Connected1");
                            self.board.replace(device);
                            break;
                        }
                        Ok(true) => {
                            log::debug!("Connected2");
                            self.board.replace(device);
                            break;
                        }
                        Err(e) => {
                            log::info!("Error checking connection, retrying: {:?}", e);
                        }
                    }
                }
                sleep(Duration::from_secs(2)).await;
            }
        }
        Ok(self.board.as_mut().unwrap())
    }
}

#[async_trait]
impl FirmwareDevice for GattBoard {
    const MTU: u32 = 4096;
    async fn version(&mut self) -> Result<String, anyhow::Error> {
        log::debug!("Reading version");
        Ok(self.read_firmware_version().await?)
    }

    async fn status(&mut self) -> Result<Option<(u32, String)>, anyhow::Error> {
        log::debug!("Status");
        let next = self.read_next_firmware_version().await?;
        let offset = self.read_firmware_offset().await?;
        Ok(Some((offset, next)))
    }

    async fn start(&mut self, version: &str) -> Result<(), anyhow::Error> {
        log::debug!("Start update");
        Ok(self.start_firmware_update(version).await?)
    }
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), anyhow::Error> {
        Ok(self.write_firmware(offset, data).await?)
    }
    async fn swap(&mut self, _: &str, _: [u8; 32]) -> Result<(), anyhow::Error> {
        log::debug!("Swapping firmware");
        let r = Ok(self.swap_firmware().await?);
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        self.adapter
            .remove_device(self.board.take().unwrap().address())
            .await?;
        self.updated = true;
        r
    }

    async fn synced(&mut self) -> Result<(), anyhow::Error> {
        if self.updated {
            log::debug!("Mark as booted");
            self.updated = false;
            Ok(self.mark_booted().await?)
        } else {
            log::debug!("Not updated?!");
            Ok(())
        }
    }
}
