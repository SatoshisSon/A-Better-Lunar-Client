//! Minimaler Leser für Minecrafts NBT-Format (Named Binary Tag, Big Endian).

use std::collections::HashMap;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Tag>),
    Compound(HashMap<String, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(map) => map.get(key),
            _ => None,
        }
    }

    /// Verschachtelter Zugriff: `tag.path(&["Data", "SpawnX"])`.
    pub fn path(&self, keys: &[&str]) -> Option<&Tag> {
        keys.iter().try_fold(self, |t, k| t.get(k))
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(v as i64),
            Tag::Short(v) => Some(v as i64),
            Tag::Int(v) => Some(v as i64),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Tag::Float(v) => Some(v as f64),
            Tag::Double(v) => Some(v),
            _ => self.as_i64().map(|v| v as f64),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_long_array(&self) -> Option<&[i64]> {
        match self {
            Tag::LongArray(v) => Some(v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

const MAX_DEPTH: usize = 512;

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or_else(|| Error::msg("NBT-Daten sind unvollständig"))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| Error::msg("Negative NBT-Länge"))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.i16()? as u16 as usize;
        // Java nutzt „modified UTF-8“; für Block- und Weltnamen reicht verlustbehaftet.
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn payload(&mut self, kind: u8, depth: usize) -> Result<Tag> {
        if depth > MAX_DEPTH {
            return Err(Error::msg("NBT zu tief verschachtelt"));
        }
        Ok(match kind {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let item = self.u8()?;
                let n = self.len()?;
                if item == 0 && n > 0 {
                    return Err(Error::msg("Ungültige NBT-Liste"));
                }
                let mut v = Vec::with_capacity(n.min(65536));
                for _ in 0..n {
                    v.push(self.payload(item, depth + 1)?);
                }
                Tag::List(v)
            }
            10 => {
                let mut map = HashMap::new();
                loop {
                    let k = self.u8()?;
                    if k == 0 {
                        break;
                    }
                    let name = self.string()?;
                    map.insert(name, self.payload(k, depth + 1)?);
                }
                Tag::Compound(map)
            }
            11 => {
                let n = self.len()?;
                let bytes = self.take(n.checked_mul(4).ok_or_else(|| Error::msg("NBT zu groß"))?)?;
                Tag::IntArray(bytes.chunks_exact(4).map(|c| i32::from_be_bytes(c.try_into().unwrap())).collect())
            }
            12 => {
                let n = self.len()?;
                let bytes = self.take(n.checked_mul(8).ok_or_else(|| Error::msg("NBT zu groß"))?)?;
                Tag::LongArray(bytes.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect())
            }
            other => return Err(Error::msg(format!("Unbekannter NBT-Typ {other}"))),
        })
    }
}

/// Liest ein unkomprimiertes NBT-Dokument (Wurzel-Compound).
pub fn parse(data: &[u8]) -> Result<Tag> {
    let mut r = Reader { data, pos: 0 };
    let kind = r.u8()?;
    if kind != 10 {
        return Err(Error::msg("NBT beginnt nicht mit einem Compound"));
    }
    r.string()?;
    r.payload(10, 0)
}

/// Liest gzip- oder zlib-komprimiertes (oder rohes) NBT.
pub fn parse_compressed(data: &[u8]) -> Result<Tag> {
    use std::io::Read;
    let mut out = Vec::new();
    match data {
        [0x1f, 0x8b, ..] => {
            flate2::read::GzDecoder::new(data).read_to_end(&mut out)?;
        }
        [0x78, ..] => {
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut out)?;
        }
        _ => return parse(data),
    }
    parse(&out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Kleiner NBT-Schreiber für Tests.
    pub fn write(name: &str, tag: &Tag) -> Vec<u8> {
        fn id(t: &Tag) -> u8 {
            match t {
                Tag::Byte(_) => 1,
                Tag::Short(_) => 2,
                Tag::Int(_) => 3,
                Tag::Long(_) => 4,
                Tag::Float(_) => 5,
                Tag::Double(_) => 6,
                Tag::ByteArray(_) => 7,
                Tag::String(_) => 8,
                Tag::List(_) => 9,
                Tag::Compound(_) => 10,
                Tag::IntArray(_) => 11,
                Tag::LongArray(_) => 12,
            }
        }
        fn s(out: &mut Vec<u8>, v: &str) {
            out.extend((v.len() as u16).to_be_bytes());
            out.extend(v.as_bytes());
        }
        fn p(out: &mut Vec<u8>, t: &Tag) {
            match t {
                Tag::Byte(v) => out.push(*v as u8),
                Tag::Short(v) => out.extend(v.to_be_bytes()),
                Tag::Int(v) => out.extend(v.to_be_bytes()),
                Tag::Long(v) => out.extend(v.to_be_bytes()),
                Tag::Float(v) => out.extend(v.to_be_bytes()),
                Tag::Double(v) => out.extend(v.to_be_bytes()),
                Tag::ByteArray(v) => {
                    out.extend((v.len() as i32).to_be_bytes());
                    out.extend(v.iter().map(|b| *b as u8));
                }
                Tag::String(v) => s(out, v),
                Tag::List(v) => {
                    out.push(v.first().map(id).unwrap_or(0));
                    out.extend((v.len() as i32).to_be_bytes());
                    v.iter().for_each(|t| p(out, t));
                }
                Tag::Compound(m) => {
                    for (k, t) in m {
                        out.push(id(t));
                        s(out, k);
                        p(out, t);
                    }
                    out.push(0);
                }
                Tag::IntArray(v) => {
                    out.extend((v.len() as i32).to_be_bytes());
                    v.iter().for_each(|x| out.extend(x.to_be_bytes()));
                }
                Tag::LongArray(v) => {
                    out.extend((v.len() as i32).to_be_bytes());
                    v.iter().for_each(|x| out.extend(x.to_be_bytes()));
                }
            }
        }
        let mut out = vec![id(tag)];
        s(&mut out, name);
        p(&mut out, tag);
        out
    }

    pub fn compound(items: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    #[test]
    fn roundtrip() {
        let tag = compound(vec![
            ("Name", Tag::String("Welt".into())),
            ("Pos", Tag::List(vec![Tag::Double(1.5), Tag::Double(64.0)])),
            ("Data", Tag::LongArray(vec![1, -2])),
            ("Inner", compound(vec![("x", Tag::Int(-7))])),
        ]);
        let bytes = write("", &tag);
        let parsed = parse(&bytes).unwrap();
        assert_eq!(parsed, tag);
        assert_eq!(parsed.path(&["Inner", "x"]).and_then(Tag::as_i64), Some(-7));
    }

    #[test]
    fn rejects_truncated() {
        let bytes = write("", &compound(vec![("a", Tag::Long(1))]));
        assert!(parse(&bytes[..bytes.len() - 3]).is_err());
    }
}
