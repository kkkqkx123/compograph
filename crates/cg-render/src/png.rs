//! Self-contained PNG encoding with stored deflate blocks.
//!
//! Rows carry a zero filter byte and travel as stored blocks inside a zlib
//! stream, with CRC32 per chunk and Adler32 over the stream. Short pixel
//! buffers are zero-padded so truncated plans still produce a valid image.

/// Encodes a flat RGB buffer as PNG with uncompressed deflate blocks.
///
/// The encoder is self-contained so image export needs no image dependency:
/// rows carry a zero filter byte and travel as stored deflate blocks inside a
/// zlib stream, with CRC32 per chunk and Adler32 over the stream. Short pixel
/// buffers are zero-padded so truncated plans still produce a valid image.
pub fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    let mut out = Vec::new();
    out.extend_from_slice(&SIGNATURE);
    let mut head = Vec::with_capacity(13);
    head.extend_from_slice(&width.to_be_bytes());
    head.extend_from_slice(&height.to_be_bytes());
    head.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut out, b"IHDR", &head);
    let row_bytes = width as usize * 3;
    let mut raw = Vec::with_capacity(height as usize * (row_bytes + 1));
    let mut rows = pixels.chunks(row_bytes.max(1));
    for _ in 0..height {
        raw.extend_from_slice(&[0]);
        match rows.next() {
            Some(bytes) => {
                raw.extend_from_slice(bytes);
                raw.resize(raw.len() + row_bytes.saturating_sub(bytes.len()), 0);
            }
            None => raw.resize(raw.len() + row_bytes, 0),
        }
    }
    push_chunk(&mut out, b"IDAT", &zlib_store(&raw));
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// Appends one PNG chunk with its length, type, data, and CRC.
fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut keyed = Vec::with_capacity(kind.len() + data.len());
    keyed.extend_from_slice(kind);
    keyed.extend_from_slice(data);
    out.extend_from_slice(&crc32(&keyed).to_be_bytes());
}

/// Wraps raw filtered rows in a zlib stream of stored deflate blocks.
///
/// Stored blocks skip compression, which keeps the encoder dependency-free;
/// each block carries at most 65535 bytes with its length complement.
fn zlib_store(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
        for (index, block) in blocks.iter().enumerate() {
            out.push(u8::from(index + 1 == blocks.len()));
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(block);
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// IEEE CRC32 over `data`, used for PNG chunk checksums.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: [u32; 256] = crc_table();
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        let slot = (crc ^ u32::from(*byte)) & 0xFF;
        crc = TABLE[slot as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Compile-time CRC table for the IEEE polynomial.
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

/// Adler32 over `data`, closing the zlib stream.
fn adler32(data: &[u8]) -> u32 {
    let mut low = 1u32;
    let mut high = 0u32;
    for byte in data {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trips_pixels_through_stored_blocks() {
        let pixels = vec![255, 0, 0, 0, 255, 0];
        let encoded = encode_png(2, 1, &pixels);
        assert_eq!(&encoded[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let chunks = split_chunks(&encoded);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].0, *b"IHDR");
        assert_eq!(&chunks[0].1[0..8], &[0, 0, 0, 2, 0, 0, 0, 1]);
        assert_eq!(chunks[1].0, *b"IDAT");
        assert_eq!(chunks[2].0, *b"IEND");
        assert!(chunks[2].1.is_empty());
        let raw = inflate_store(&chunks[1].1);
        assert_eq!(raw, vec![0, 255, 0, 0, 0, 255, 0]);
    }

    #[test]
    fn png_header_matches_dimensions() {
        let encoded = encode_png(3, 2, &[9; 18]);
        let chunks = split_chunks(&encoded);
        assert_eq!(&chunks[0].1[0..8], &[0, 0, 0, 3, 0, 0, 0, 2]);
        let raw = inflate_store(&chunks[1].1);
        assert_eq!(raw.len(), 2 * (3 * 3 + 1));
    }

    /// Splits an encoded image into (kind, data) pairs, checking CRCs.
    fn split_chunks(encoded: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut chunks = Vec::new();
        let mut cursor = 8;
        while cursor + 8 <= encoded.len() {
            let len = u32::from_be_bytes(encoded[cursor..cursor + 4].try_into().unwrap_or([0; 4]))
                as usize;
            let kind: [u8; 4] = encoded[cursor + 4..cursor + 8].try_into().unwrap_or([0; 4]);
            let data = encoded[cursor + 8..cursor + 8 + len].to_vec();
            let stored = u32::from_be_bytes(
                encoded[cursor + 8 + len..cursor + 12 + len]
                    .try_into()
                    .unwrap_or([0; 4]),
            );
            let mut keyed = Vec::new();
            keyed.extend_from_slice(&kind);
            keyed.extend_from_slice(&data);
            assert_eq!(stored, crc32(&keyed));
            chunks.push((kind, data));
            cursor += 12 + len;
            if kind == *b"IEND" {
                break;
            }
        }
        chunks
    }

    /// Decodes the stored-block zlib stream the encoder writes.
    fn inflate_store(stream: &[u8]) -> Vec<u8> {
        assert_eq!(&stream[0..2], &[0x78, 0x01]);
        let mut raw = Vec::new();
        let mut cursor = 2;
        loop {
            let final_block = stream[cursor] & 0x01 == 0x01;
            assert_eq!(stream[cursor] >> 1, 0);
            let len = u16::from_le_bytes([stream[cursor + 1], stream[cursor + 2]]) as usize;
            let complement = u16::from_le_bytes([stream[cursor + 3], stream[cursor + 4]]);
            assert_eq!(complement, !len as u16);
            raw.extend_from_slice(&stream[cursor + 5..cursor + 5 + len]);
            cursor += 5 + len;
            if final_block {
                break;
            }
        }
        let stored = u32::from_be_bytes(stream[cursor..cursor + 4].try_into().unwrap_or([0; 4]));
        assert_eq!(stored, adler32(&raw));
        raw
    }
}
