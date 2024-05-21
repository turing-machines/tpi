// Copyright 2024 Turing Machines
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use core::panic;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
    u8,
};

use crate::utils;
use anyhow::{bail, Context};
use bytes::{BufMut, Bytes, BytesMut};
use rusb::GlobalContext;
use tar::Archive;
use tokio::sync::watch;

const BMC_VENDOR: u16 = 0x0006;
const BMC_PRODUCT: u16 = 0x0011;

struct HotplugMonitor {
    sender: watch::Sender<Option<rusb::Device<GlobalContext>>>,
}

impl HotplugMonitor {
    fn new(sender: watch::Sender<Option<rusb::Device<GlobalContext>>>) -> Self {
        Self { sender }
    }
}

impl rusb::Hotplug<rusb::GlobalContext> for HotplugMonitor {
    fn device_arrived(&mut self, device: rusb::Device<rusb::GlobalContext>) {
        self.sender
            .send(Some(device))
            .expect("HotplugMonitor cannot outlive watch channel");
    }

    fn device_left(&mut self, _device: rusb::Device<rusb::GlobalContext>) {
        self.sender
            .send(None)
            .expect("HotplugMonitor cannot outlive watch channel");
    }
}

pub async fn get_fel_deviec(default_host: bool) -> anyhow::Result<()> {
    let (sender, mut watcher) = watch::channel(None);
    let _hotplug = rusb::HotplugBuilder::new()
        .vendor_id(BMC_VENDOR)
        .product_id(BMC_PRODUCT)
        .enumerate(true)
        .register(
            rusb::GlobalContext::default(),
            Box::new(HotplugMonitor::new(sender)),
        )?;

    let spinner = utils::build_spinner();
    spinner.set_message("waiting for BMC to go into FEL..");
    watcher.changed().await;
    todo!()
}

fn find_tpi_devices() -> anyhow::Result<Vec<rusb::Device<GlobalContext>>> {
    Ok(rusb::devices()?
        .iter()
        .filter_map(|d| {
            let desc = d.device_descriptor().ok()?;
            (desc.vendor_id() == BMC_VENDOR && desc.product_id() == BMC_PRODUCT).then_some(d)
        })
        .collect())
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
enum FelParts {
    Bootloader = 0,
    Rootfs = 1,
    EEPROM = 2,
    Bootscript = 3,
    Initramfs = 4,
}

impl TryFrom<u8> for FelParts {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FelParts::Bootloader),
            1 => Ok(FelParts::Rootfs),
            2 => Ok(FelParts::EEPROM),
            3 => Ok(FelParts::Bootscript),
            4 => Ok(FelParts::Initramfs),
            _ => bail!("unknown felparts id `{}`", value),
        }
    }
}

fn untar_content_txt(file: impl Read) -> anyhow::Result<HashMap<String, FelParts>> {
    let mut archive = Archive::new(file);
    let contents_file = archive
        .entries()?
        .find(|e| {
            e.as_ref()
                .is_ok_and(|ent| ent.header().path().unwrap().eq(Path::new("contents.txt")))
        })
        .context("missing contents.txt inside tar archive")??;

    let mut content_map = HashMap::new();
    let buf_reader = BufReader::new(contents_file);
    for line in buf_reader.lines() {
        let line = line?;
        let Some((r#type, name)) = line.split_once(',') else {
            println!("contents.txt parse error: missing ',' on line `{}`", line);
            continue;
        };

        let fel_part = FelParts::try_from(r#type.parse::<u8>().context(name.to_string())?)?;
        content_map.insert(name.trim().to_string(), fel_part);
    }
    Ok(content_map)
}

fn unpack_tar() -> anyhow::Result<HashMap<FelParts, Bytes>> {
    let path = PathBuf::from("/home/svenr/turing-pi/buildroot/output/images/fel_upgrade.tpf");
    let mut file = File::open(path)?;
    let contents_map = untar_content_txt(&file).context("untar contents.txt")?;
    file.seek(SeekFrom::Start(0))?;
    let mut archive = Archive::new(file);

    let mut results = HashMap::new();
    for install_part in archive.entries()? {
        let mut part = install_part?;
        let path = part.header().path()?;
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !contents_map.contains_key(&name) {
            println!(
                "skipping `{}` as its not present in the contents.txt file",
                &name
            );
            continue;
        }

        let mut bytes = BytesMut::with_capacity(part.size() as usize);
        part.read_exact(bytes.as_mut())?;
        results.insert(contents_map[&name], bytes.into());
    }

    Ok(results)
}

pub async fn flash_usb() -> anyhow::Result<()> {
    let devices = find_tpi_devices()?;
    let device = devices.first().unwrap();
    let handle = device.open()?;
    handle.claim_interface(0)?;

    let parts = unpack_tar()?;
    println!("{:?}", parts);

    if let Some(part) = parts.get(&FelParts::Bootloader) {
        println!("bootloader");
        let mut bytes = BytesMut::with_capacity(9);
        bytes.put_u8(FelParts::Bootloader as u8);
        bytes.put_u64(part.len() as u64);

        handle.write_bulk(0x1, &bytes, Duration::from_secs(5))?;
        handle.write_bulk(0x1, part, Duration::from_secs(5))?;
    }

    panic!("{:?}", handle);
}
