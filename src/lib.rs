pub mod backend;
pub mod bitmap;
pub mod cli;
pub mod client;
pub mod protocol;
pub mod server;

pub const DEFAULT_BLOCK_SIZE: u64 = 4096;

pub fn mib(value: u64) -> u64 {
    value * 1024 * 1024
}
