//! HTTP/1.1 message framing shared by the server and client adapters
//! (RFC 9112): strict chunked decoding with bounded, validated trailers,
//! Content-Length parsing and field-section measurement.

use tokio::io::{AsyncRead, AsyncReadExt};

/// Longest chunk-size line (size plus extensions) accepted.
const MAX_CHUNK_LINE: usize = 8192;

/// Why a framed body could not be read.
pub enum ChunkErr<E> {
    /// Invalid framing or premature end of the stream.
    Framing,
    /// The declared size would exceed `max_total` (no data was read for it).
    TooLarge,
    /// The trailer section exceeded `header_limit` (a received field section).
    TrailerTooLarge,
    /// The consumer rejected a piece of data.
    Sink(E),
}

fn is_ows(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

pub fn trim_ows(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if is_ows(*first) {
            s = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = s {
        if is_ows(*last) {
            s = rest;
        } else {
            break;
        }
    }
    s
}

/// `1*HEXDIG` value; numerals longer than 16 significant digits saturate to
/// `u64::MAX` (RFC 9110 §8.6 / RFC 9112 §7.1: no overflow errors).
fn hex_saturating(digits: &[u8]) -> u64 {
    let sig: &[u8] = {
        let first = digits.iter().position(|&c| c != b'0').unwrap_or(digits.len());
        &digits[first..]
    };
    if sig.len() > 16 {
        return u64::MAX;
    }
    sig.iter().fold(0u64, |n, &c| n * 16 + (c as char).to_digit(16).unwrap() as u64)
}

/// `chunk-size [ chunk-ext ]` (RFC 9112 §7.1.1):
/// `1*HEXDIG *( BWS ";" BWS ext-name [ BWS "=" BWS ( token / quoted-string ) ] )`.
pub fn parse_chunk_size(line: &[u8]) -> Option<u64> {
    use crate::builtins::headers::{is_token_char, quoted_string_len};
    let hex_end = line.iter().position(|c| !c.is_ascii_hexdigit()).unwrap_or(line.len());
    if hex_end == 0 {
        return None;
    }
    let size = hex_saturating(&line[..hex_end]);
    let bws = |mut k: usize| {
        while k < line.len() && is_ows(line[k]) {
            k += 1;
        }
        k
    };
    let token_end = |k: usize| {
        let mut e = k;
        while e < line.len() && is_token_char(line[e]) {
            e += 1;
        }
        e
    };
    let mut k = hex_end;
    while k < line.len() {
        k = bws(k);
        if line.get(k) != Some(&b';') {
            return None;
        }
        k = bws(k + 1);
        let e = token_end(k);
        if e == k {
            return None;
        }
        k = e;
        let after = bws(k);
        if line.get(after) == Some(&b'=') {
            k = bws(after + 1);
            if line.get(k) == Some(&b'"') {
                k += quoted_string_len(&line[k..])?;
            } else {
                let e = token_end(k);
                if e == k {
                    return None;
                }
                k = e;
            }
        }
    }
    Some(size)
}

/// Content-Length per RFC 9110 §8.6: `1*DIGIT`, possibly a comma-separated list
/// of identical values. `Ok(None)` when no field is present; numerals beyond
/// `u64` saturate to `u64::MAX`; invalid or conflicting values are `Err`.
#[allow(clippy::result_unit_err)] // the only failure is "invalid or conflicting"
pub fn content_length<'a>(values: impl Iterator<Item = &'a [u8]>) -> Result<Option<u64>, ()> {
    let mut seen: Option<Vec<u8>> = None;
    let mut value = 0u64;
    for v in values {
        for part in v.split(|&c| c == b',') {
            let p = trim_ows(part);
            if p.is_empty() || !p.iter().all(|c| c.is_ascii_digit()) {
                return Err(());
            }
            let first = p.iter().position(|&c| c != b'0').unwrap_or(p.len() - 1);
            let norm = p[first..].to_vec();
            if seen.as_ref().is_some_and(|s| *s != norm) {
                return Err(());
            }
            value = norm.iter().fold(0u64, |n, &d| n.saturating_mul(10).saturating_add((d - b'0') as u64));
            seen = Some(norm);
        }
    }
    Ok(seen.map(|_| value))
}

/// Octets of the field section of a message head ending at `head_end`
/// (index after the final CRLF CRLF): the field lines with their CRLFs,
/// excluding the start line and the terminal empty line.
pub fn field_section_len(head: &[u8], head_end: usize) -> usize {
    match head.windows(2).position(|w| w == b"\r\n") {
        Some(le) if le + 2 <= head_end => head_end.saturating_sub(le + 2 + 2),
        _ => 0,
    }
}

/// Buffered source over an async reader; consumed bytes are dropped so
/// memory stays bounded.
struct Src<'a, R> {
    rd: &'a mut R,
    buf: Vec<u8>,
    pos: usize,
}

impl<R: AsyncRead + Unpin> Src<'_, R> {
    async fn fill(&mut self) -> Result<(), ()> {
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let mut chunk = [0u8; 16384];
        match self.rd.read(&mut chunk).await {
            Ok(0) | Err(_) => Err(()),
            Ok(k) => {
                self.buf.extend_from_slice(&chunk[..k]);
                Ok(())
            }
        }
    }

    /// One CRLF-terminated line without the CRLF.
    async fn line(&mut self) -> Result<Vec<u8>, ()> {
        loop {
            if let Some(i) = self.buf[self.pos..].windows(2).position(|w| w == b"\r\n") {
                let l = self.buf[self.pos..self.pos + i].to_vec();
                self.pos += i + 2;
                return Ok(l);
            }
            if self.buf.len() - self.pos > MAX_CHUNK_LINE {
                return Err(());
            }
            self.fill().await?;
        }
    }

    async fn expect_crlf(&mut self) -> Result<(), ()> {
        while self.buf.len() - self.pos < 2 {
            self.fill().await?;
        }
        if &self.buf[self.pos..self.pos + 2] != b"\r\n" {
            return Err(());
        }
        self.pos += 2;
        Ok(())
    }
}

/// Decode a chunked body. Data is delivered to `sink` in pieces as it
/// arrives. The decoded total may not exceed `max_total` (checked against each
/// declared chunk size *before* reading it); the trailer section is a received
/// field section bounded by `header_limit` and its fields are validated.
pub async fn read_chunked<R, E>(
    rd: &mut R,
    initial: Vec<u8>,
    max_total: u64,
    header_limit: u64,
    mut sink: impl FnMut(&[u8]) -> Result<(), E>,
) -> Result<(), ChunkErr<E>>
where
    R: AsyncRead + Unpin,
{
    let mut src = Src { rd, buf: initial, pos: 0 };
    let mut total: u64 = 0;
    loop {
        let l = src.line().await.map_err(|_| ChunkErr::Framing)?;
        let size = parse_chunk_size(&l).ok_or(ChunkErr::Framing)?;
        if size == 0 {
            let mut trailer_bytes: u64 = 0;
            loop {
                let t = match src.line().await {
                    Ok(t) => t,
                    // an over-long line is itself an over-limit trailer section
                    Err(()) if src.buf.len() - src.pos > MAX_CHUNK_LINE => return Err(ChunkErr::TrailerTooLarge),
                    Err(()) => return Err(ChunkErr::Framing),
                };
                if t.is_empty() {
                    return Ok(());
                }
                trailer_bytes += t.len() as u64 + 2;
                if trailer_bytes > header_limit {
                    return Err(ChunkErr::TrailerTooLarge);
                }
                // trailer-section = *( field-line CRLF ) (RFC 9112 §7.1.2)
                let ok = match t.iter().position(|&c| c == b':') {
                    Some(i) => {
                        let name = std::str::from_utf8(&t[..i]).unwrap_or("");
                        crate::builtins::headers::valid_field(name, &t[i + 1..])
                    }
                    None => false,
                };
                if !ok {
                    return Err(ChunkErr::Framing);
                }
            }
        }
        if size > max_total.saturating_sub(total) {
            return Err(ChunkErr::TooLarge);
        }
        let mut left = size;
        while left > 0 {
            if src.pos == src.buf.len() {
                src.fill().await.map_err(|_| ChunkErr::Framing)?;
            }
            let avail = ((src.buf.len() - src.pos) as u64).min(left) as usize;
            sink(&src.buf[src.pos..src.pos + avail]).map_err(ChunkErr::Sink)?;
            src.pos += avail;
            left -= avail as u64;
        }
        total += size;
        src.expect_crlf().await.map_err(|_| ChunkErr::Framing)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_size_lines() {
        assert_eq!(parse_chunk_size(b"7"), Some(7));
        assert_eq!(parse_chunk_size(b"7;ext"), Some(7));
        assert_eq!(parse_chunk_size(b"7 ; a=b"), Some(7));
        assert_eq!(parse_chunk_size(b"7;a=\"q\\\"x\""), Some(7));
        assert_eq!(parse_chunk_size(b"00000000000000005"), Some(5));
        assert_eq!(parse_chunk_size(b"ffffffffffffffffff"), Some(u64::MAX));
        for bad in [&b" 7"[..], b"7 ", b"7;\x01", b"7;", b"7;=b", b"+7", b"", b"7;a=", b"7;a=\"x"] {
            assert_eq!(parse_chunk_size(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn content_lengths() {
        let cl = |v: &[&[u8]]| content_length(v.iter().copied());
        assert_eq!(cl(&[]), Ok(None));
        assert_eq!(cl(&[b"5"]), Ok(Some(5)));
        assert_eq!(cl(&[b"5, 5", b" 05 "]), Ok(Some(5)));
        assert_eq!(cl(&[b"5", b"6"]), Err(()));
        assert_eq!(cl(&[b""]), Err(()));
        assert_eq!(cl(&[b"+5"]), Err(()));
        assert_eq!(cl(&[b"99999999999999999999999"]), Ok(Some(u64::MAX)));
        assert_eq!(cl(&[b"99999999999999999999999", b"99999999999999999999998"]), Err(()));
    }

    #[test]
    fn field_section() {
        let head = b"GET / HTTP/1.1\r\nA: b\r\nC: d\r\n\r\n";
        assert_eq!(field_section_len(head, head.len()), "A: b\r\nC: d\r\n".len());
        let none = b"GET / HTTP/1.1\r\n\r\n";
        assert_eq!(field_section_len(none, none.len()), 0);
    }

    #[tokio::test]
    async fn chunked_decoding() {
        let run = |data: &'static [u8], max: u64| async move {
            let mut rd = tokio::io::BufReader::new(data);
            let mut out = Vec::new();
            let r = read_chunked::<_, ()>(&mut rd, Vec::new(), max, 64, |d| {
                out.extend_from_slice(d);
                Ok(())
            })
            .await;
            r.map(|_| out).map_err(|e| match e {
                ChunkErr::Framing => "framing",
                ChunkErr::TooLarge => "large",
                ChunkErr::TrailerTooLarge => "trailer",
                ChunkErr::Sink(()) => "sink",
            })
        };
        assert_eq!(run(b"3\r\nabc\r\n2;x\r\nde\r\n0\r\nT: v\r\n\r\n", 100).await, Ok(b"abcde".to_vec()));
        assert_eq!(run(b"3\r\nabc\r\n0\r\n\r\n", 2).await, Err("large"));
        assert_eq!(run(b"3\r\nabcX\r\n0\r\n\r\n", 100).await, Err("framing"));
        assert_eq!(run(b"3\r\nabc\r\n0\r\nBad Trailer\r\n\r\n", 100).await, Err("framing"));
        assert_eq!(run(b"3\r\nab", 100).await, Err("framing"));
        assert_eq!(run(b"0\r\nX: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n", 100).await, Err("trailer"));
    }
}
