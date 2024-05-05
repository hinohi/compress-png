use std::{borrow::Cow, collections::HashMap, ffi::OsString, fs};

use clap::Parser;
use itertools::Itertools;
use png::{BitDepth, ColorType, Compression, Decoder, Encoder, FilterType, Transformations};

trait IterPixel {
    fn iter_ga(&self) -> impl Iterator<Item=(u8, u8)>;

    fn iter_rgb(&self) -> impl Iterator<Item=(u8, u8, u8)>;

    fn iter_rgba(&self) -> impl Iterator<Item=(u8, u8, u8, u8)>;
}

impl IterPixel for [u8] {
    fn iter_ga(&self) -> impl Iterator<Item=(u8, u8)> {
        self.iter().copied().tuples()
    }

    fn iter_rgb(&self) -> impl Iterator<Item=(u8, u8, u8)> {
        self.iter().copied().tuples()
    }

    fn iter_rgba(&self) -> impl Iterator<Item=(u8, u8, u8, u8)> {
        self.iter().copied().tuples()
    }
}

#[derive(Parser)]
struct Opts {
    src: OsString,
}


fn main() -> std::io::Result<()> {
    let opts = Opts::parse();
    let src_data = fs::read(&opts.src)?;

    let mut decoder = Decoder::new(src_data.as_slice());
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bytes = &buf[..info.buffer_size()];
    println!("{:?}", info);

    let (trivial_compressed, color) = trivial_compress(bytes, info.color_type);
    let (pallet_compressed, pallet, color, bit_depth) = calc_pallet(&trivial_compressed, color);

    let mut best_size = usize::MAX;
    let mut best_out = Vec::new();
    for f in [FilterType::NoFilter, FilterType::Sub, FilterType::Up, FilterType::Avg, FilterType::Paeth] {
        let out = encode(&pallet_compressed, info.width, info.height, color, pallet.as_ref(), bit_depth, f);
        println!("filter={:?} size={}", f, out.len());
        if out.len() < best_size {
            best_size = out.len();
            best_out = out;
        }
    }
    fs::write("out.png", &best_out)?;
    Ok(())
}

fn trivial_compress(data: &[u8], color: ColorType) -> (Cow<'_, [u8]>, ColorType) {
    match color {
        ColorType::Grayscale => {
            (Cow::Borrowed(data), ColorType::Grayscale)
        }
        ColorType::Rgb => {
            let mut gray = Vec::new();
            for (r, g, b) in data.iter_rgb() {
                if r == g && r == b {
                    gray.push(r);
                } else {
                    return (Cow::Borrowed(data), ColorType::Rgb);
                }
            }
            (Cow::Owned(gray), ColorType::Grayscale)
        }
        ColorType::Indexed => unreachable!(),
        ColorType::GrayscaleAlpha => {
            let mut gray = Vec::new();
            for (g, a) in data.iter_ga() {
                if a == 0xFF {
                    gray.push(g);
                } else {
                    return (Cow::Borrowed(data), ColorType::GrayscaleAlpha);
                }
            }
            (Cow::Owned(gray), ColorType::Grayscale)
        }
        ColorType::Rgba => {
            if data.iter().skip(3).step_by(4).any(|&a| a != 0xFF) {
                return (Cow::Borrowed(data), ColorType::Rgba);
            }
            if data.iter_rgba().all(|(r, g, b, _)| r == g && r == b) {
                let data = data.iter().step_by(4).copied().collect::<Vec<_>>();
                return (Cow::Owned(data), ColorType::Grayscale);
            }
            let mut rgb = Vec::with_capacity(data.len() * 3 / 4);
            for (r, g, b, _) in data.iter_rgba() {
                rgb.push(r);
                rgb.push(g);
                rgb.push(b);
            }
            (Cow::Owned(rgb), ColorType::Rgb)
        }
    }
}

fn calc_pallet(data: &[u8], color: ColorType) -> (Cow<'_, [u8]>, Option<Vec<u8>>, ColorType, BitDepth) {
    match color {
        ColorType::Grayscale | ColorType::GrayscaleAlpha | ColorType::Rgba | ColorType::Indexed => {
            (Cow::Borrowed(data), None, color, BitDepth::Eight)
        }
        ColorType::Rgb => {
            let mut count = HashMap::new();
            for rgb in data.iter_rgb() {
                *count.entry(rgb).or_insert(0u32) += 1;
            }
            eprintln!("colors={}", count.len());
            if count.len() > 256 {
                return (Cow::Borrowed(data), None, color, BitDepth::Eight);
            }
            let mut count = count.into_iter().collect::<Vec<_>>();
            count.sort_unstable_by(|a, b| b.1.cmp(&a.1));
            let pallet_map = count.iter().enumerate().map(|(i, x)| (x.0, i as u8)).collect::<HashMap<_, _>>();
            let mut pallet = Vec::with_capacity(count.len() * 3);
            for &((r, g, b), _) in count.iter() {
                pallet.push(r);
                pallet.push(g);
                pallet.push(b);
            }
            let buf = data.iter_rgb().map(|rgb| pallet_map[&rgb]).collect();
            (Cow::Owned(buf), Some(pallet), ColorType::Indexed, BitDepth::Eight)
        }
    }
}

fn encode(bytes: &[u8], width: u32, height: u32, color_type: ColorType, pallet: Option<&Vec<u8>>, bit_depth: BitDepth, filter_type: FilterType) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = Encoder::new(&mut buf, width, height);
        encoder.set_compression(Compression::Best);
        encoder.set_color(color_type);
        if let Some(pallet) = pallet {
            encoder.set_palette(pallet);
        }
        encoder.set_depth(bit_depth);
        encoder.set_filter(filter_type);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(bytes).unwrap();
    }
    buf
}
