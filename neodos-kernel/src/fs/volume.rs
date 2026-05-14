#![allow(dead_code)]

use crate::buffer::block_cache::BlockCache;
use crate::drivers::ata::{AtaChannel, AtaDriver};
use crate::fs::neodos_fs::{FsError, NeoDosFs};

pub const MAX_VOLUMES: usize = 4;

pub struct Volume {
    pub fs: NeoDosFs,
    pub cache: BlockCache,
    pub base_lba: u32,
    pub channel: AtaChannel,
}

impl Volume {
    pub fn from_partition(ata: &mut AtaDriver, base_lba: u32) -> Result<Self, FsError> {
        let channel = ata.channel();
        ata.set_base_lba(base_lba);
        let sb_data = ata.read_sector(0)?;
        let mut fs = NeoDosFs::new(&sb_data)?;
        let mut cache = BlockCache::new();
        fs.rebuild_bitmap(&mut cache, ata)?;
        Ok(Volume { fs, cache, base_lba, channel })
    }
}
