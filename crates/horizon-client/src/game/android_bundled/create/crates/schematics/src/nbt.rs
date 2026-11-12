//! Small, bounded Java-edition named NBT codec, written for this port.
//!
//! Format contract: big-endian primitive tags 0..12 and Java DataInput/DataOutput
//! modified UTF-8 strings. No Mojang implementation is copied. Gzip is handled
//! by the already-used MIT/Apache-2.0 `flate2` crate. Keeping this explicit reader
//! avoids an unbounded recursive deserializer and preserves numeric tag types,
//! empty-list element types and floating-point bits for the native adapter.
use super::{Error, Limits};
use flate2::{Compression, bufread::GzDecoder, write::GzEncoder};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};

pub type Compound = BTreeMap<String, Tag>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TagKind {
    End = 0,
    Byte = 1,
    Short = 2,
    Int = 3,
    Long = 4,
    Float = 5,
    Double = 6,
    ByteArray = 7,
    String = 8,
    List = 9,
    Compound = 10,
    IntArray = 11,
    LongArray = 12,
}
impl TagKind {
    fn read(id: u8) -> Result<Self, Error> {
        Ok(match id {
            0 => Self::End,
            1 => Self::Byte,
            2 => Self::Short,
            3 => Self::Int,
            4 => Self::Long,
            5 => Self::Float,
            6 => Self::Double,
            7 => Self::ByteArray,
            8 => Self::String,
            9 => Self::List,
            10 => Self::Compound,
            11 => Self::IntArray,
            12 => Self::LongArray,
            _ => return Err(Error::new("nbt", format!("unknown tag type {id}"))),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    ByteArray(Vec<i8>),
    String(String),
    List { element: TagKind, values: Vec<Tag> },
    Compound(Compound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}
impl Tag {
    pub fn kind(&self) -> TagKind {
        match self {
            Self::Byte(_) => TagKind::Byte,
            Self::Short(_) => TagKind::Short,
            Self::Int(_) => TagKind::Int,
            Self::Long(_) => TagKind::Long,
            Self::Float(_) => TagKind::Float,
            Self::Double(_) => TagKind::Double,
            Self::ByteArray(_) => TagKind::ByteArray,
            Self::String(_) => TagKind::String,
            Self::List { .. } => TagKind::List,
            Self::Compound(_) => TagKind::Compound,
            Self::IntArray(_) => TagKind::IntArray,
            Self::LongArray(_) => TagKind::LongArray,
        }
    }
    pub fn list(element: TagKind, values: Vec<Tag>) -> Self {
        Self::List { element, values }
    }
    pub fn as_compound(&self) -> Option<&Compound> {
        if let Self::Compound(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    nodes: usize,
    limits: &'a Limits,
}
impl Reader<'_> {
    fn error(&self, message: impl Into<String>) -> Error {
        Error::new(format!("nbt@{}", self.offset), message)
    }
    fn take(&mut self, size: usize) -> Result<&[u8], Error> {
        let end = self
            .offset
            .checked_add(size)
            .ok_or_else(|| self.error("length overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| self.error("truncated payload"))?;
        self.offset = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn int(&mut self) -> Result<i32, Error> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn count(&mut self) -> Result<usize, Error> {
        let count = self.int()?;
        let count = usize::try_from(count).map_err(|_| self.error("negative collection length"))?;
        if count > self.limits.max_collection_len {
            return Err(self.error("collection length exceeds limit"));
        }
        Ok(count)
    }
    fn string(&mut self) -> Result<String, Error> {
        let length = u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as usize;
        if length > self.limits.max_string_bytes {
            return Err(self.error("string length exceeds limit"));
        }
        decode_modified_utf8(self.take(length)?).map_err(|message| self.error(message))
    }
    fn payload(&mut self, kind: TagKind, depth: usize) -> Result<Tag, Error> {
        if depth > self.limits.max_depth {
            return Err(self.error("depth exceeds limit"));
        }
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(self.error("node count exceeds limit"));
        }
        Ok(match kind {
            TagKind::End => return Err(self.error("unexpected end tag")),
            TagKind::Byte => Tag::Byte(self.byte()? as i8),
            TagKind::Short => Tag::Short(i16::from_be_bytes(self.take(2)?.try_into().unwrap())),
            TagKind::Int => Tag::Int(self.int()?),
            TagKind::Long => Tag::Long(i64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            TagKind::Float => Tag::Float(u32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            TagKind::Double => Tag::Double(u64::from_be_bytes(self.take(8)?.try_into().unwrap())),
            TagKind::String => Tag::String(self.string()?),
            TagKind::ByteArray => {
                let count = self.count()?;
                Tag::ByteArray(self.take(count)?.iter().map(|b| *b as i8).collect())
            }
            TagKind::IntArray => {
                let count = self.count()?;
                let bytes = self.take(
                    count
                        .checked_mul(4)
                        .ok_or_else(|| Error::new("nbt", "array length overflow"))?,
                )?;
                Tag::IntArray(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| i32::from_be_bytes(*b))
                        .collect(),
                )
            }
            TagKind::LongArray => {
                let count = self.count()?;
                let bytes = self.take(
                    count
                        .checked_mul(8)
                        .ok_or_else(|| Error::new("nbt", "array length overflow"))?,
                )?;
                Tag::LongArray(
                    bytes
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|b| i64::from_be_bytes(*b))
                        .collect(),
                )
            }
            TagKind::List => {
                let element = TagKind::read(self.byte()?)?;
                let count = self.count()?;
                if element == TagKind::End && count != 0 {
                    return Err(self.error("nonempty end-tag list"));
                }
                if count > self.limits.max_nodes.saturating_sub(self.nodes) {
                    return Err(self.error("list exceeds node budget"));
                }
                let mut values = Vec::new();
                // Do not allocate from an untrusted count before reading its payload.
                for _ in 0..count {
                    values.push(self.payload(element, depth + 1)?);
                }
                Tag::List { element, values }
            }
            TagKind::Compound => {
                let mut compound = Compound::new();
                loop {
                    let kind = TagKind::read(self.byte()?)?;
                    if kind == TagKind::End {
                        break;
                    }
                    if compound.len() >= self.limits.max_collection_len {
                        return Err(self.error("compound size exceeds limit"));
                    }
                    let name = self.string()?;
                    let value = self.payload(kind, depth + 1)?;
                    if compound.insert(name.clone(), value).is_some() {
                        return Err(self.error(format!("duplicate compound key {name:?}")));
                    }
                }
                Tag::Compound(compound)
            }
        })
    }
}

/// Strict single-member gzip + named compound NBT. Trailing bytes are rejected.
pub fn decode_gzip(bytes: &[u8], limits: &Limits) -> Result<Compound, Error> {
    if bytes.len() > limits.max_compressed_bytes {
        return Err(Error::new("gzip", "compressed size exceeds limit"));
    }
    let mut decoder = GzDecoder::new(bytes);
    let mut raw = Vec::new();
    decoder
        .by_ref()
        .take(limits.max_uncompressed_bytes.saturating_add(1) as u64)
        .read_to_end(&mut raw)
        .map_err(|e| Error::new("gzip", e.to_string()))?;
    if raw.len() > limits.max_uncompressed_bytes {
        return Err(Error::new("gzip", "decompressed size exceeds limit"));
    }
    if !decoder.get_ref().is_empty() {
        return Err(Error::new("gzip", "trailing data or multiple gzip members"));
    }
    decode_named(&raw, limits)
}

pub fn decode_named(bytes: &[u8], limits: &Limits) -> Result<Compound, Error> {
    if bytes.len() > limits.max_uncompressed_bytes {
        return Err(Error::new("nbt", "size exceeds limit"));
    }
    let mut reader = Reader {
        bytes,
        offset: 0,
        nodes: 0,
        limits,
    };
    if reader.byte()? != TagKind::Compound as u8 {
        return Err(reader.error("root must be a named compound"));
    }
    let _name = reader.string()?;
    let root = reader.payload(TagKind::Compound, 0)?;
    if reader.offset != bytes.len() {
        return Err(reader.error("trailing NBT bytes"));
    }
    if let Tag::Compound(root) = root {
        Ok(root)
    } else {
        unreachable!()
    }
}

/// A fixture/export codec, also bounded by the same limits as the reader.
pub fn encode_gzip(root: &Compound, limits: &Limits) -> Result<Vec<u8>, Error> {
    let bytes = encode_named(root, limits)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&bytes)
        .map_err(|e| Error::new("gzip", e.to_string()))?;
    let bytes = encoder
        .finish()
        .map_err(|e| Error::new("gzip", e.to_string()))?;
    if bytes.len() > limits.max_compressed_bytes {
        return Err(Error::new("gzip", "compressed size exceeds limit"));
    }
    Ok(bytes)
}

pub fn encode_named(root: &Compound, limits: &Limits) -> Result<Vec<u8>, Error> {
    struct Writer<'a> {
        out: Vec<u8>,
        nodes: usize,
        limits: &'a Limits,
    }
    impl Writer<'_> {
        fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
            if bytes.len()
                > self
                    .limits
                    .max_uncompressed_bytes
                    .saturating_sub(self.out.len())
            {
                return Err(Error::new("nbt", "encoded size exceeds limit"));
            }
            self.out.extend_from_slice(bytes);
            Ok(())
        }
        fn string(&mut self, text: &str) -> Result<(), Error> {
            let bytes = encode_modified_utf8(text);
            let length = u16::try_from(bytes.len())
                .map_err(|_| Error::new("nbt", "string exceeds u16 length"))?;
            if bytes.len() > self.limits.max_string_bytes {
                return Err(Error::new("nbt", "string length exceeds limit"));
            }
            self.write(&length.to_be_bytes())?;
            self.write(&bytes)
        }
        fn count(&mut self, count: usize) -> Result<(), Error> {
            if count > self.limits.max_collection_len {
                return Err(Error::new("nbt", "collection length exceeds limit"));
            }
            let count = i32::try_from(count)
                .map_err(|_| Error::new("nbt", "collection exceeds i32 length"))?;
            self.write(&count.to_be_bytes())
        }
        fn tag(&mut self, tag: &Tag, depth: usize) -> Result<(), Error> {
            self.nodes += 1;
            if self.nodes > self.limits.max_nodes || depth > self.limits.max_depth {
                return Err(Error::new("nbt", "node/depth limit exceeded"));
            }
            match tag {
                Tag::Byte(v) => self.write(&[*v as u8]),
                Tag::Short(v) => self.write(&v.to_be_bytes()),
                Tag::Int(v) => self.write(&v.to_be_bytes()),
                Tag::Long(v) => self.write(&v.to_be_bytes()),
                Tag::Float(v) => self.write(&v.to_be_bytes()),
                Tag::Double(v) => self.write(&v.to_be_bytes()),
                Tag::String(v) => self.string(v),
                Tag::ByteArray(v) => {
                    self.count(v.len())?;
                    for x in v {
                        self.write(&[*x as u8])?;
                    }
                    Ok(())
                }
                Tag::IntArray(v) => {
                    self.count(v.len())?;
                    for x in v {
                        self.write(&x.to_be_bytes())?;
                    }
                    Ok(())
                }
                Tag::LongArray(v) => {
                    self.count(v.len())?;
                    for x in v {
                        self.write(&x.to_be_bytes())?;
                    }
                    Ok(())
                }
                Tag::List { element, values } => {
                    self.write(&[*element as u8])?;
                    self.count(values.len())?;
                    for value in values {
                        if value.kind() != *element {
                            return Err(Error::new("nbt", "mixed list tag types"));
                        }
                        self.tag(value, depth + 1)?;
                    }
                    Ok(())
                }
                Tag::Compound(v) => {
                    if v.len() > self.limits.max_collection_len {
                        return Err(Error::new("nbt", "compound size exceeds limit"));
                    }
                    for (name, value) in v {
                        self.write(&[value.kind() as u8])?;
                        self.string(name)?;
                        self.tag(value, depth + 1)?;
                    }
                    self.write(&[0])
                }
            }
        }
    }
    let mut writer = Writer {
        out: Vec::new(),
        nodes: 0,
        limits,
    };
    writer.write(&[10, 0, 0])?;
    writer.tag(&Tag::Compound(root.clone()), 0)?;
    Ok(writer.out)
}

pub fn encode_modified_utf8(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for unit in text.encode_utf16() {
        match unit {
            1..=0x7f => bytes.push(unit as u8),
            0..=0x7ff => {
                bytes.push((0xc0 | unit >> 6) as u8);
                bytes.push((0x80 | unit & 63) as u8);
            }
            _ => {
                bytes.push((0xe0 | unit >> 12) as u8);
                bytes.push((0x80 | unit >> 6 & 63) as u8);
                bytes.push((0x80 | unit & 63) as u8);
            }
        }
    }
    bytes
}
pub fn decode_modified_utf8(bytes: &[u8]) -> Result<String, &'static str> {
    let mut units = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let first = bytes[i];
        i += 1;
        if first <= 0x7f {
            units.push(u16::from(first));
            continue;
        }
        let (count, mut value) = match first {
            0xc0..=0xdf => (1, u16::from(first & 31)),
            0xe0..=0xef => (2, u16::from(first & 15)),
            _ => return Err("invalid modified UTF-8 lead byte"),
        };
        for _ in 0..count {
            let next = *bytes.get(i).ok_or("truncated modified UTF-8")?;
            i += 1;
            if next & 0xc0 != 0x80 {
                return Err("invalid modified UTF-8 continuation");
            }
            value = value << 6 | u16::from(next & 63);
        }
        if (count == 1 && value < 0x80 && value != 0) || (count == 2 && value < 0x800) {
            return Err("overlong modified UTF-8");
        }
        units.push(value);
    }
    String::from_utf16(&units).map_err(|_| "unpaired UTF-16 surrogate is unsupported")
}
