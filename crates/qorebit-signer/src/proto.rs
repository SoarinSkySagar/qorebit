//! Just enough protobuf for Cosmos transactions: a field walker and canonical (proto3: zero and
//! empty fields omitted) encoders, matching what `@qorechain/qoresign` writes byte for byte.

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Varint,
    Fixed64,
    Len,
    Fixed32,
}

/// One top-level field of a message.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub num: u32,
    pub wire: Wire,
    pub varint: u64,
    /// The payload of a length-delimited field (empty otherwise).
    pub bytes: &'a [u8],
    /// The whole field as it appears on the wire (tag included).
    pub raw: &'a [u8],
}

pub fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..10 {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if i == 9 && b > 1 {
            return None;
        }
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// All top-level fields, in order. `None` for anything malformed (truncated, wire types 3/4,
/// field number 0): the device refuses to sign what it can't read completely.
pub fn fields(buf: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let start = pos;
        let tag = read_varint(buf, &mut pos)?;
        let num = u32::try_from(tag >> 3).ok().filter(|&n| n != 0)?;
        let mut f = Field { num, wire: Wire::Varint, varint: 0, bytes: &[], raw: &[] };
        match tag & 7 {
            0 => f.varint = read_varint(buf, &mut pos)?,
            1 => {
                f.wire = Wire::Fixed64;
                pos = pos.checked_add(8).filter(|&p| p <= buf.len())?;
            }
            2 => {
                f.wire = Wire::Len;
                let n = usize::try_from(read_varint(buf, &mut pos)?).ok()?;
                let end = pos.checked_add(n).filter(|&e| e <= buf.len())?;
                f.bytes = &buf[pos..end];
                pos = end;
            }
            5 => {
                f.wire = Wire::Fixed32;
                pos = pos.checked_add(4).filter(|&p| p <= buf.len())?;
            }
            _ => return None,
        }
        f.raw = &buf[start..pos];
        out.push(f);
    }
    Some(out)
}

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// A length-delimited field; empty payloads are omitted (proto3).
pub fn put_len(out: &mut Vec<u8>, num: u32, payload: &[u8]) {
    if payload.is_empty() {
        return;
    }
    put_varint(out, (num as u64) << 3 | 2);
    put_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

/// A varint field; zero is omitted (proto3).
pub fn put_uint(out: &mut Vec<u8>, num: u32, v: u64) {
    if v != 0 {
        put_varint(out, (num as u64) << 3);
        put_varint(out, v);
    }
}

/// `google.protobuf.Any`: type URL (1), value (2).
pub fn any(type_url: &str, value: &[u8]) -> Vec<u8> {
    let mut o = Vec::new();
    put_len(&mut o, 1, type_url.as_bytes());
    put_len(&mut o, 2, value);
    o
}

/// The first field `num` of a message, if any.
pub fn find<'a>(fs: &[Field<'a>], num: u32) -> Option<&'a [u8]> {
    fs.iter().find(|f| f.num == num && f.wire == Wire::Len).map(|f| f.bytes)
}

pub fn utf8(b: &[u8]) -> Option<&str> {
    core::str::from_utf8(b).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trip() {
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut o = Vec::new();
            put_varint(&mut o, v);
            let mut p = 0;
            assert_eq!(read_varint(&o, &mut p), Some(v));
            assert_eq!(p, o.len());
        }
    }

    #[test]
    fn rejects_truncated_and_odd_wire_types() {
        assert!(fields(&[0x0a, 5, 1, 2]).is_none()); // length runs past the end
        assert!(fields(&[0x0b]).is_none()); // start-group
        assert!(fields(&[0x00, 1]).is_none()); // field number 0
        assert!(fields(&[0x08]).is_none()); // varint missing
        assert_eq!(fields(&[]).unwrap().len(), 0);
    }

    #[test]
    fn empty_and_zero_fields_are_omitted() {
        let mut o = Vec::new();
        put_len(&mut o, 1, b"");
        put_uint(&mut o, 2, 0);
        assert!(o.is_empty());
        put_uint(&mut o, 2, 7);
        assert_eq!(o, [0x10, 7]);
    }
}
