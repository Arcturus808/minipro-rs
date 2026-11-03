//! Small shared helpers ported from upstream C minipro.

/// Ported from `crc_32` in upstream `minipro.c`: reflected CRC-32
/// (polynomial `0xEDB88320`) with a caller-supplied initial value and
/// **no final complement**.
///
/// XGecu firmware images and `.alg` bitstreams embed CRCs computed this
/// way — do not substitute `CRC_32_ISO_HDLC`, which XORs the result with
/// `0xFFFFFFFF` and will always mismatch.
pub(crate) fn crc_32(data: &[u8], initial: u32) -> u32 {
    let mut crc = initial;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_32_matches_known_value() {
        // Standard CRC-32 of "123456789" is 0xCBF43926; without the final
        // complement it is its bitwise NOT.
        assert_eq!(crc_32(b"123456789", 0xFFFF_FFFF), 0x340B_C6D9);
    }
}
