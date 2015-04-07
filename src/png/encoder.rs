//! A PNG Encoder
//!
//! This implementation uses the ```flate``` crate for compression
//! and selects the filter type using the sum of absolute differences method.
//!
//! For each row the filter method that produces the lowest integer when its bytes
//! are interpreted as signed numbers and summed is chosen as the filter.

use std::slice;
use std::io;
use std::io::Write;
use num::FromPrimitive;
use std::iter::repeat;
use byteorder::{WriteBytesExt, BigEndian};

use color;
use super::hash::Crc32;

use super::filter::filter;
use super::decoder::PNGSIGNATURE;
use super::zlib::ZlibEncoder;

/// The representation of a PNG encoder
pub struct PNGEncoder<'a, W: 'a> {
    w: &'a mut W,
    crc: Crc32
}

impl<'a, W: Write> PNGEncoder<'a, W> {
    /// Create a new encoder that writes its output to ```w```
    pub fn new(w: &mut W) -> PNGEncoder<W> {
        PNGEncoder {
            w: w,
            crc: Crc32::new()
        }
    }

    /// Encodes the image ```image```
    /// that has dimensions ```width``` and ```height```
    /// and ```ColorType``` ```c```
    pub fn encode(&mut self,
                  image: &[u8],
                  width: u32,
                  height: u32,
                  c: color::ColorType) -> io::Result<()> {

        let _ = try!(self.write_signature());
        let (bytes, bpp) = build_ihdr(width, height, c);

        let _ = try!(self.write_chunk("IHDR", &bytes));
        let compressed_bytes = try!(build_idat(image, bpp, width));

        for chunk in compressed_bytes.chunks(1024 * 256) {
            let _ = try!(self.write_chunk("IDAT", chunk));
        }

        self.write_chunk("IEND", &[])
    }

    fn write_signature(&mut self) -> io::Result<()> {
        self.w.write_all(&PNGSIGNATURE)
    }

    fn write_chunk(&mut self, name: &str, buf: &[u8]) -> io::Result<()> {
        self.crc.reset();
        self.crc.update(name.as_bytes());
        self.crc.update(&buf);

        let crc = self.crc.checksum();

        let _ = try!(self.w.write_u32::<BigEndian>(buf.len() as u32));
        let _ = try!(write!(self.w, "{}", name));
        let _ = try!(self.w.write_all(buf));
        let _ = try!(self.w.write_u32::<BigEndian>(crc));

        Ok(())
    }
}

fn build_ihdr(width: u32, height: u32, c: color::ColorType) -> (Vec<u8>, usize) {
    let mut m = Vec::with_capacity(13);

    let _ = m.write_u32::<BigEndian>(width);
    let _ = m.write_u32::<BigEndian>(height);

    let (colortype, bit_depth) = match c {
        color::ColorType::Gray(1)    => (0, 1),
        color::ColorType::Gray(2)    => (0, 2),
        color::ColorType::Gray(4)    => (0, 4),
        color::ColorType::Gray(8)    => (0, 8),
        color::ColorType::Gray(16)   => (0, 16),
        color::ColorType::RGB(8)     => (2, 8),
        color::ColorType::RGB(16)    => (2, 16),
        color::ColorType::Palette(1) => (3, 1),
        color::ColorType::Palette(2) => (3, 2),
        color::ColorType::Palette(4) => (3, 4),
        color::ColorType::Palette(8) => (3, 8),
        color::ColorType::GrayA(8)   => (4, 8),
        color::ColorType::GrayA(16)  => (4, 16),
        color::ColorType::RGBA(8)    => (6, 8),
        color::ColorType::RGBA(16)   => (6, 16),
        _ => panic!("unsupported color type and bitdepth")
    };

    let _ = m.write_all(&[bit_depth]);
    let _ = m.write_all(&[colortype]);

    // Compression method, filter method and interlace
    let _ = m.write_all(&[0]);
    let _ = m.write_all(&[0]);
    let _ = m.write_all(&[0]);

    let channels = match colortype {
        0 => 1,
        2 => 3,
        3 => 3,
        4 => 2,
        6 => 4,
        _ => panic!("unknown colour type")
    };

    let bpp = ((channels * bit_depth + 7) / 8) as usize;

    (m, bpp)
}

fn sum_abs_difference(buf: &[u8]) -> i32 {
    buf.iter().fold(0i32, | sum, &b | sum + if b < 128 {b as i32} else {256 - b as i32})
}

fn select_filter(rowlength: usize, bpp: usize, previous: &[u8], current_s: &mut [u8]) -> u8 {
    let mut sum    = sum_abs_difference(&current_s[..rowlength]);
    let mut method = 0;

    for (i, current) in current_s.chunks_mut(rowlength).enumerate() {
        filter(FromPrimitive::from_u8(i as u8 + 1).unwrap(), bpp, previous, current);

        let this_sum = sum_abs_difference(current);

        if this_sum < sum {
            sum = this_sum;
            method = i as u8 + 1;
        }
    }

    method
}

fn build_idat(image: &[u8], bpp: usize, width: u32) -> io::Result<Vec<u8>> {
    let rowlen = bpp * width as usize;

    let mut p: Vec<u8> = repeat(0u8).take(rowlen).collect();
    let mut c: Vec<u8> = repeat(0u8).take(4 * rowlen).collect();
    let mut e = try!(ZlibEncoder::new(Vec::new()));

    for row in image.chunks(rowlen) {
        for s in c.chunks_mut(rowlen) {
            slice::bytes::copy_memory(row, s);
        }

        let filter = select_filter(rowlen, bpp, &p, &mut c);

        try!(e.write_all(&[filter]));

        match filter {
            0 => try!(e.write_all(row)),
            _ => {
                let stride = (filter as usize - 1) * rowlen;
                try!(e.write_all(&c[stride..stride + rowlen]))
            }
        }

        slice::bytes::copy_memory(row, &mut p);
    }

    e.into_inner()
}
