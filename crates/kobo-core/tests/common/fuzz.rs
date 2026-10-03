//! Dependency-free mutation smoke fuzzing shared by CI and the longer driver.
//! BPS patches are among the inputs.
//! Reproduce a case with its seed; this is not coverage-guided fuzzing.

use kobo_core::level::objects::{self, Jumps, Layout};
use kobo_core::mwl::{Layer2Data, Mwl, MwlFile};
use kobo_core::{Rom, SnesAddr, bps, compress, gfx, sprites};

/// A small MWL file in Lunar Magic's layout: objects on both layers
/// (mode 1), two sprites, one entrance, some ExAnimation bytes.
fn mwl_seed() -> Vec<u8> {
    let with_header = |b0: u8, data: &[u8]| {
        let mut out = vec![b0, 0, 0, 0, 0x00, 0x80, 0x10, 0];
        out.extend_from_slice(data);
        out
    };
    let mut info = vec![0x05, 0x01, 0x5B, 0x00, 0x9A, 0x00];
    info.resize(0x40, 0);
    let objects = [
        0x33, 0x01, 0x08, 0x80, 0x27, 0x0A, 0x53, 0x12, 0x98, 0xD2, 0x00, 0x03, 0x00, 0x01, 0x05,
        0x04, 0x2D, 0x04, 0x03, 0x00, 0x21, 0xFF,
    ];
    let sections = [
        info,
        with_header(0, &objects),
        with_header(0, &objects),
        with_header(
            0,
            &[
                0x20, 0x31, 0x61, 0x35, 0xFF, 0x01, 0x40, 0x81, 0x99, 0xFF, 0xFE,
            ],
        ),
        with_header(0, &[0x11; 514]),
        with_header(0, &[0xCB, 0x01, 0xA9, 0x08, 0x0E, 0, 0, 0]),
        with_header(0, &[0x02, 0x00, 0xFF, 0xFF, 0, 0]),
        vec![0x7F; 32],
    ];
    MwlFile {
        version: 0x0370,
        flags: [0; 4],
        comment: [b' '; 48],
        sections: sections.to_vec(),
    }
    .to_bytes()
}

/// The level without its encoded lengths, which the encoder chooses.
fn without_lengths(mut mwl: Mwl) -> Mwl {
    mwl.layer1.data.len = 0;
    if let Layer2Data::Objects(data) = &mut mwl.layer2.data {
        data.len = 0;
    }
    mwl.sprites.list.len = 0;
    mwl
}

pub fn case(mut seed: u64) {
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let len = next() as usize % 2048;
    let input: Vec<u8> = (0..len).map(|_| next() as u8).collect();
    for decode in [
        compress::lz2::decompress,
        compress::lz3::decompress,
        compress::rle1::decompress,
    ] {
        for n in [0, input.len() / 2, input.len()] {
            if let Ok(result) = decode(&input[..n]) {
                assert!(result.consumed <= n);
                assert!(result.data.len() <= compress::MAX_OUTPUT);
            }
        }
    }
    // What decodes encodes again to the same thing, where it can be placed.
    for layout in [Layout::Horizontal, Layout::Vertical] {
        for jumps in [Jumps::Vanilla, Jumps::Tall] {
            if let Ok(data) = objects::decode(&input, layout, jumps) {
                assert!(data.len <= input.len());
                if let Ok(bytes) = objects::encode(data.header, &data.objects, layout, jumps) {
                    let again = objects::decode(&bytes, layout, jumps).unwrap();
                    assert_eq!(again.objects, data.objects);
                }
            }
        }
    }
    if let Ok(result) = compress::rle1::decompress(&input)
        && (1..=4096).contains(&result.data.len())
    {
        let packed = compress::rle1::compress(&result.data).unwrap();
        assert_eq!(
            compress::rle1::decompress(&packed).unwrap().data,
            result.data
        );
    }
    // LC_LZ2 round trips, of the noise and of runs and repeats built from
    // it, never larger than storing the bytes as they are.
    let mut shaped = Vec::new();
    while shaped.len() < len {
        let run = 1 + next() as usize % 80;
        let b = next() as u8;
        match next() % 4 {
            0 => shaped.extend(input.iter().take(run)),
            1 => shaped.extend(std::iter::repeat_n(b, run)),
            2 => shaped.extend((0..run).map(|k| b.wrapping_add(k as u8))),
            _ => {
                let from = next() as usize % (shaped.len() + 1);
                let copy: Vec<u8> = shaped[from..].iter().cycle().take(run).copied().collect();
                shaped.extend(copy);
            }
        }
    }
    for data in [&input, &shaped] {
        let packed = compress::lz2::compress(data).unwrap();
        let back = compress::lz2::decompress(&packed).unwrap();
        assert_eq!(back.data, *data);
        assert_eq!(back.consumed, packed.len());
        assert!(packed.len() <= data.len() + 2 * data.len().div_ceil(1024) + 1);
    }
    if let Ok(list) = sprites::decode(&input, None) {
        assert!(list.len <= input.len());
    }
    // An MWL file with a few bytes changed and maybe cut short: what
    // decodes encodes to a file that decodes the same.
    let mut mwl = mwl_seed();
    let changes = next() % 8;
    for _ in 0..changes {
        let at = next() as usize % mwl.len();
        mwl[at] = next() as u8;
    }
    if next() % 4 == 0 {
        mwl.truncate(next() as usize % mwl.len());
    } else if changes == 0 {
        assert!(MwlFile::parse(&mwl).and_then(|f| f.decode(None)).is_ok());
    }
    for bytes in [&mwl, &input] {
        if let Ok(level) = MwlFile::parse(bytes).and_then(|f| f.decode(None))
            && let Ok(file) = level.to_file(None)
        {
            let again = file.decode(None).unwrap();
            assert!(without_lengths(again) == without_lengths(level));
        }
    }
    // Valid container shape, arbitrary headers and pointer operands. Without
    // a structured seed, almost every ROM mutation is rejected at BadSize.
    let mut bytes = vec![0; 0x8000];
    bytes[..input.len()].copy_from_slice(&input);
    for byte in &mut bytes[0x3800..0x3A30] {
        *byte = next() as u8;
    }
    for byte in &mut bytes[0x7FC0..] {
        *byte = next() as u8;
    }
    bytes[0x7FD5] = if next() & 1 == 0 { 0x20 } else { 0x23 };
    let rom = Rom::from_bytes(bytes).unwrap();
    let header = rom.internal_header();
    let _ = header.rom_size();
    let _ = header.sram_size();
    let _ = rom.compute_checksum();
    let addr = SnesAddr::new(next() as u32);
    let _ = rom.read(addr, usize::MAX);
    let _ = rom.read_tail(addr);
    let _ = gfx::read_gfx_file(&rom, next() as u8);
    let _ = sprites::read_sprites_at(&rom, SnesAddr::new(0x008000));
    let _ = sprites::read_sprites_at(&rom, addr);

    // A BPS patch between two overlapping, edited halves of the input
    // round trips. Mutated, with its own CRC fixed up so the actions are
    // reached, or truncated, it is an error or some target, never a panic.
    let source = &input[..input.len() / 2];
    let mut target = input[input.len() / 4..].to_vec();
    for _ in 0..next() % 8 {
        if !target.is_empty() {
            let i = next() as usize % target.len();
            target[i] = next() as u8;
        }
    }
    let patch = bps::create(source, &target);
    assert_eq!(bps::apply(&patch, source).as_deref(), Ok(&target[..]));
    let mut mutated = patch.clone();
    for _ in 0..=next() % 4 {
        let i = next() as usize % mutated.len();
        mutated[i] = next() as u8;
    }
    let _ = bps::apply(&mutated, source);
    let n = mutated.len();
    let crc = bps::crc32(&mutated[..n - 4]);
    mutated[n - 4..].copy_from_slice(&crc.to_le_bytes());
    let _ = bps::apply(&mutated, source);
    let _ = bps::apply(&patch[..next() as usize % patch.len()], source);
}
