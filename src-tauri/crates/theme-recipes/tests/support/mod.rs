pub fn synthetic_png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    append_png_chunk(&mut bytes, b"IHDR", &ihdr);

    let scanline = [0_u8, 0x21, 0x43, 0x65, 0xff];
    let mut compressed = vec![0x78, 0x01, 0x01];
    let length = u16::try_from(scanline.len()).expect("small PNG scanline");
    compressed.extend_from_slice(&length.to_le_bytes());
    compressed.extend_from_slice(&(!length).to_le_bytes());
    compressed.extend_from_slice(&scanline);
    compressed.extend_from_slice(&adler32(&scanline).to_be_bytes());
    append_png_chunk(&mut bytes, b"IDAT", &compressed);
    append_png_chunk(&mut bytes, b"IEND", &[]);
    bytes
}

pub fn synthetic_ogg() -> Vec<u8> {
    let payload = b"original-synthetic-ogg-packet";
    let mut page = vec![0_u8; 28];
    page[..4].copy_from_slice(b"OggS");
    page[5] = 0x06;
    page[14..18].copy_from_slice(&0x4531_0001_u32.to_le_bytes());
    page[26] = 1;
    page[27] = u8::try_from(payload.len()).expect("small Ogg packet");
    page.extend_from_slice(payload);
    let checksum = ogg_checksum(&page);
    page[22..26].copy_from_slice(&checksum.to_le_bytes());
    page
}

fn append_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], payload: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("synthetic PNG chunk length")
            .to_be_bytes(),
    );
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(payload);
    output.extend_from_slice(&png_crc(chunk_type, payload).to_be_bytes());
}

fn png_crc(chunk_type: &[u8; 4], payload: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in chunk_type.iter().chain(payload) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    !crc
}

fn ogg_checksum(bytes: &[u8]) -> u32 {
    let mut checksum = 0_u32;
    for byte in bytes {
        checksum ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            checksum = if checksum & 0x8000_0000 == 0 {
                checksum << 1
            } else {
                (checksum << 1) ^ 0x04c1_1db7
            };
        }
    }
    checksum
}

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let mut first = 1_u32;
    let mut second = 0_u32;
    for byte in bytes {
        first = (first + u32::from(*byte)) % MODULUS;
        second = (second + first) % MODULUS;
    }
    (second << 16) | first
}
