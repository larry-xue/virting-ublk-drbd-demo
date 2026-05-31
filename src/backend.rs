use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug)]
pub struct FileBackend {
    path: PathBuf,
    file: Mutex<File>,
    size: u64,
}

impl FileBackend {
    pub fn open(path: impl AsRef<Path>, size: u64) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        let current_len = file.metadata()?.len();
        if current_len == 0 {
            file.set_len(size)?;
            file.sync_all()?;
        } else if current_len != size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "backing file {} has size {}, expected {}",
                    path.display(),
                    current_len,
                    size
                ),
            ));
        }

        Ok(Self {
            path,
            file: Mutex::new(file),
            size,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        self.check_range(offset, len as u64)?;
        let mut file = self.lock_file()?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn write_at(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.check_range(offset, data.len() as u64)?;
        let mut file = self.lock_file()?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        file.sync_data()
    }

    pub fn checksum(&self) -> io::Result<u64> {
        let mut file = self.lock_file()?;
        file.seek(SeekFrom::Start(0))?;

        let mut checksum = FNV_OFFSET;
        let mut buf = [0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            checksum = checksum_bytes(checksum, &buf[..n]);
        }
        Ok(checksum)
    }

    fn lock_file(&self) -> io::Result<std::sync::MutexGuard<'_, File>> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("backend file mutex poisoned"))
    }

    fn check_range(&self, offset: u64, len: u64) -> io::Result<()> {
        let end = offset.checked_add(len).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "offset + len overflows u64")
        })?;
        if end > self.size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("range {offset}..{end} exceeds device size {}", self.size),
            ));
        }
        Ok(())
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub fn checksum_bytes(mut state: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        state ^= u64::from(*b);
        state = state.wrapping_mul(FNV_PRIME);
    }
    state
}
