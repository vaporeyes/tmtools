// ABOUTME: LZO1X decompression (the "safe" decoder), which TMF uses for Gbx file bodies, and a stored encoder that
// ABOUTME: wraps data as one literal run any LZO1X decoder reads back. Decoding follows lzo1x_decompress_safe from minilzo.

use crate::{format_err, Result};

enum State {
    Loop,
    FirstLiteralRun,
    Match,
    MatchDone,
    MatchNext,
}

struct Lzo<'a> {
    src: &'a [u8],
    out: Vec<u8>,
    out_len: usize,
    ip: usize,
}

impl Lzo<'_> {
    fn byte(&mut self) -> Result<usize> {
        let Some(&b) = self.src.get(self.ip) else { return format_err("LZO input overrun") };
        self.ip += 1;
        Ok(b as usize)
    }

    /// Run lengths extend with zero bytes worth 255 each, then a final byte
    fn extend(&mut self, base: usize) -> Result<usize> {
        let mut length = 0;
        while self.src.get(self.ip) == Some(&0) {
            length += 255;
            self.ip += 1;
        }
        Ok(length + base + self.byte()?)
    }

    fn literals(&mut self, count: usize) -> Result<()> {
        if self.out.len() + count > self.out_len || self.ip + count > self.src.len() {
            return format_err("LZO literal overrun");
        }
        self.out.extend_from_slice(&self.src[self.ip..self.ip + count]);
        self.ip += count;
        Ok(())
    }

    /// Matches may overlap their own output, so copy byte by byte
    fn copy_match(&mut self, distance_back: usize, count: usize) -> Result<()> {
        let Some(from) = self.out.len().checked_sub(distance_back) else { return format_err("LZO lookbehind overrun") };
        if self.out.len() + count > self.out_len {
            return format_err("LZO output overrun");
        }
        for i in 0..count {
            let b = self.out[from + i];
            self.out.push(b);
        }
        Ok(())
    }

    fn two_byte_offset(&mut self) -> Result<usize> {
        let lo = self.byte()?;
        let hi = self.byte()?;
        Ok((lo >> 2) + (hi << 6))
    }
}

pub fn lzo1x_decompress(src: &[u8], out_len: usize) -> Result<Vec<u8>> {
    let mut z = Lzo { src, out: Vec::with_capacity(out_len), out_len, ip: 0 };
    let mut t: usize = 0;
    let mut state = State::Loop;

    if src.first().is_some_and(|&b| b > 17) {
        t = z.byte()? - 17;
        if t < 4 {
            state = State::MatchNext;
        } else {
            z.literals(t)?;
            state = State::FirstLiteralRun;
        }
    }

    loop {
        state = match state {
            State::Loop => {
                t = z.byte()?;
                if t >= 16 {
                    State::Match
                } else {
                    if t == 0 {
                        t = z.extend(15)?;
                    }
                    z.literals(t + 3)?;
                    State::FirstLiteralRun
                }
            }
            State::FirstLiteralRun => {
                t = z.byte()?;
                if t >= 16 {
                    State::Match
                } else {
                    let back = 1 + 0x0800 + (t >> 2) + (z.byte()? << 2);
                    z.copy_match(back, 3)?;
                    State::MatchDone
                }
            }
            State::Match => {
                if t >= 64 {
                    let back = 1 + ((t >> 2) & 7) + (z.byte()? << 3);
                    z.copy_match(back, (t >> 5) - 1 + 2)?;
                } else if t >= 32 {
                    t &= 31;
                    if t == 0 {
                        t = z.extend(31)?;
                    }
                    let back = 1 + z.two_byte_offset()?;
                    z.copy_match(back, t + 2)?;
                } else if t >= 16 {
                    let high = (t & 8) << 11;
                    t &= 7;
                    if t == 0 {
                        t = z.extend(7)?;
                    }
                    let back = high + z.two_byte_offset()?;
                    // A distance of zero marks the end of the stream
                    if back == 0 {
                        if z.out.len() != out_len {
                            return format_err(format!("LZO stream ended at {} of {out_len} bytes", z.out.len()));
                        }
                        return Ok(z.out);
                    }
                    z.copy_match(back + 0x4000, t + 2)?;
                } else {
                    let back = 1 + (t >> 2) + (z.byte()? << 2);
                    z.copy_match(back, 2)?;
                }
                State::MatchDone
            }
            State::MatchDone => {
                // The low two bits of the match's first offset byte say how many literals follow
                t = z.src[z.ip - 2] as usize & 3;
                if t == 0 {
                    State::Loop
                } else {
                    State::MatchNext
                }
            }
            State::MatchNext => {
                z.literals(t)?;
                t = z.byte()?;
                State::Match
            }
        };
    }
}

/// Data as a valid LZO1X stream without compressing it: one literal run, then the end-of-stream marker. Decoders
/// read a run of up to 18 bytes from its first byte, and longer runs from a zero byte, zero bytes worth 255 more each,
/// and a final count.
pub fn lzo1x_store(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut out = Vec::with_capacity(n + n / 255 + 8);
    match n {
        0 => {}
        // The first byte says how many literals follow, up to 238 with the "first byte above 17" form
        1..=238 => out.push((n + 17) as u8),
        _ => {
            // Literal count t + 3, with t = 15 + 255 * zeros + final byte (1 to 255)
            let mut rest = n - 3 - 15;
            out.push(0);
            while rest > 255 {
                out.push(0);
                rest -= 255;
            }
            out.push(rest as u8);
        }
    }
    out.extend_from_slice(data);
    // End of stream: a match instruction with distance zero
    out.extend_from_slice(&[0x11, 0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_streams_decode_to_the_input_at_every_length() {
        for n in [1usize, 3, 17, 18, 19, 237, 238, 239, 240, 255, 273, 274, 275, 528, 529, 530, 70_000] {
            let data: Vec<u8> = (0..n).map(|i| (i * 7 % 251) as u8).collect();
            assert_eq!(lzo1x_decompress(&lzo1x_store(&data), n).unwrap(), data, "length {n}");
        }
    }

    // A first byte above 17 starts with that many literals (minus 17); 0x11 0x00 0x00 ends the stream
    #[test]
    fn decodes_a_literal_run() {
        let src = [17 + 5, b'h', b'e', b'l', b'l', b'o', 0x11, 0, 0];
        assert_eq!(lzo1x_decompress(&src, 5).unwrap(), b"hello");
    }

    // After "abcd", match byte 0x4C (t >= 64): length (0x4C >> 5) + 1 = 3, distance 1 + ((0x4C >> 2) & 7) + 0 = 4
    #[test]
    fn decodes_a_short_match() {
        let src = [17 + 4, b'a', b'b', b'c', b'd', 0x4C, 0x00, 0x11, 0, 0];
        assert_eq!(lzo1x_decompress(&src, 7).unwrap(), b"abcdabc");
    }

    #[test]
    fn rejects_streams_that_end_early() {
        let src = [17 + 5, b'h', b'e', b'l', b'l', b'o', 0x11, 0, 0];
        assert!(lzo1x_decompress(&src, 6).is_err());
        assert!(lzo1x_decompress(&src[..4], 5).is_err());
    }
}
