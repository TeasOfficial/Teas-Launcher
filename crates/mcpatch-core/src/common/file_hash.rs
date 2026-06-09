//! 文件哈希计算（CRC64 + CRC16 组合哈希）

use crc::{Crc, CRC_16_IBM_SDLC, CRC_64_XZ};
use tokio::io::{AsyncRead, AsyncReadExt};

/// 同步计算文件哈希（CRC64_XZ + CRC16_IBM_SDLC 组合）
pub fn calculate_hash(read: &mut impl std::io::Read) -> String {
    let crc64 = Crc::<u64>::new(&CRC_64_XZ);
    let mut crc64 = crc64.digest();
    let crc16 = Crc::<u16>::new(&CRC_16_IBM_SDLC);
    let mut crc16 = crc16.digest();
    let mut buffer = [0u8; 16 * 1024];

    loop {
        let count = read.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        crc64.update(&buffer[0..count]);
        crc16.update(&buffer[0..count]);
    }

    format!("{:016x}_{:04x}", crc64.finalize(), crc16.finalize())
}

/// 异步计算文件哈希（CRC64_XZ + CRC16_IBM_SDLC 组合）
pub async fn calculate_hash_async(read: &mut (impl AsyncRead + Unpin)) -> String {
    let crc64 = Crc::<u64>::new(&CRC_64_XZ);
    let mut crc64 = crc64.digest();
    let crc16 = Crc::<u16>::new(&CRC_16_IBM_SDLC);
    let mut crc16 = crc16.digest();
    let mut buffer = [0u8; 16 * 1024];

    loop {
        let count = read.read(&mut buffer).await.unwrap();
        if count == 0 {
            break;
        }
        crc64.update(&buffer[0..count]);
        crc16.update(&buffer[0..count]);
    }

    format!("{:016x}_{:04x}", crc64.finalize(), crc16.finalize())
}
