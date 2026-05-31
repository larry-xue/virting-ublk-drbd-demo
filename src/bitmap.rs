use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const BITMAP_MAGIC: [u8; 8] = *b"VRBDBM1!";

#[derive(Debug)]
pub struct DirtyBitmap {
    path: PathBuf,
    block_size: u64,
    block_count: u64,
    words: Vec<u64>,
}

impl DirtyBitmap {
    pub fn load_or_create(
        path: impl AsRef<Path>,
        block_size: u64,
        device_size: u64,
    ) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let block_count = device_size.div_ceil(block_size);
        let word_count = block_count.div_ceil(64) as usize;

        if path.exists() {
            let mut file = OpenOptions::new().read(true).open(&path)?;
            let mut header = [0u8; 32];
            file.read_exact(&mut header)?;
            if header[0..8] != BITMAP_MAGIC {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid dirty bitmap magic in {}", path.display()),
                ));
            }
            let stored_block_size = read_u64(&header[8..16]);
            let stored_block_count = read_u64(&header[16..24]);
            let stored_words = read_u64(&header[24..32]) as usize;
            if stored_block_size != block_size
                || stored_block_count != block_count
                || stored_words != word_count
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("dirty bitmap geometry mismatch in {}", path.display()),
                ));
            }

            let mut words = vec![0u64; word_count];
            for word in &mut words {
                let mut bytes = [0u8; 8];
                file.read_exact(&mut bytes)?;
                *word = u64::from_be_bytes(bytes);
            }
            Ok(Self {
                path,
                block_size,
                block_count,
                words,
            })
        } else {
            let bitmap = Self {
                path,
                block_size,
                block_count,
                words: vec![0u64; word_count],
            };
            bitmap.save()?;
            Ok(bitmap)
        }
    }

    pub fn block_size(&self) -> u64 {
        self.block_size
    }

    pub fn dirty_count(&self) -> u64 {
        self.words.iter().map(|word| word.count_ones() as u64).sum()
    }

    pub fn dirty_blocks(&self) -> Vec<u64> {
        let mut blocks = Vec::new();
        for block in 0..self.block_count {
            if self.is_dirty(block) {
                blocks.push(block);
            }
        }
        blocks
    }

    pub fn mark_all(&mut self) {
        for word in &mut self.words {
            *word = u64::MAX;
        }
        self.mask_unused_tail_bits();
    }

    pub fn mark_range(&mut self, offset: u64, len: u64) {
        self.set_range(offset, len, true);
    }

    pub fn clear_range(&mut self, offset: u64, len: u64) {
        self.set_range(offset, len, false);
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)?;
        file.write_all(&BITMAP_MAGIC)?;
        file.write_all(&self.block_size.to_be_bytes())?;
        file.write_all(&self.block_count.to_be_bytes())?;
        file.write_all(&(self.words.len() as u64).to_be_bytes())?;
        for word in &self.words {
            file.write_all(&word.to_be_bytes())?;
        }
        file.sync_all()
    }

    fn is_dirty(&self, block: u64) -> bool {
        let word = (block / 64) as usize;
        let bit = block % 64;
        (self.words[word] & (1u64 << bit)) != 0
    }

    fn set_range(&mut self, offset: u64, len: u64, dirty: bool) {
        if len == 0 {
            return;
        }
        let start = offset / self.block_size;
        let end = (offset + len).div_ceil(self.block_size);
        for block in start..end.min(self.block_count) {
            let word = (block / 64) as usize;
            let bit = block % 64;
            if dirty {
                self.words[word] |= 1u64 << bit;
            } else {
                self.words[word] &= !(1u64 << bit);
            }
        }
    }

    fn mask_unused_tail_bits(&mut self) {
        let used_tail = self.block_count % 64;
        if used_tail == 0 || self.words.is_empty() {
            return;
        }
        let mask = (1u64 << used_tail) - 1;
        if let Some(last) = self.words.last_mut() {
            *last &= mask;
        }
    }
}

fn read_u64(bytes: &[u8]) -> u64 {
    let mut array = [0u8; 8];
    array.copy_from_slice(bytes);
    u64::from_be_bytes(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_marks_cover_partial_blocks() {
        let path = std::env::temp_dir().join(format!(
            "vrbd-bitmap-test-{}-{}.bm",
            std::process::id(),
            unique_nanos()
        ));
        let mut bm = DirtyBitmap::load_or_create(&path, 4096, 16 * 4096).unwrap();
        bm.mark_range(4095, 2);
        assert_eq!(bm.dirty_blocks(), vec![0, 1]);
        bm.clear_range(4096, 4096);
        assert_eq!(bm.dirty_blocks(), vec![0]);
        let _ = std::fs::remove_file(path);
    }

    fn unique_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
