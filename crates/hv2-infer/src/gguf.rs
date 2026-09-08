//! Reading a GGUF file, which is where the weights actually are.
//!
//! GGUF is a header of typed key/value metadata, a table of tensor descriptors,
//! and then one contiguous run of tensor data. Everything this crate needs to
//! know about a model — how many layers, how many heads, what the RoPE base is,
//! the whole vocabulary and every merge rule — is in the metadata, so a model
//! file is self-describing and nothing here has an architecture compiled into
//! it.
//!
//! # Mapped, not read
//!
//! [`Gguf::open`] maps the file and never copies the tensor data. That is not a
//! performance nicety, it is the arrangement this project measured before it
//! had anything to put in it: a fleet of agents sharing one read-only mapping
//! of the weights costs the model once rather than once each, and 1,000 agents
//! against a 350 MiB region saved 99.85% of what copying would have cost.
//!
//! # What is deliberately not here
//!
//! Writing. Nothing in this repository produces a GGUF, and a parser that can
//! only read is a parser that cannot corrupt a 1.3 GB file by accident.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use memmap2::Mmap;

/// What went wrong reading a model file.
#[derive(Debug)]
pub enum Error {
    /// The file could not be opened or mapped.
    Io(std::io::Error),
    /// The first four bytes are not `GGUF`.
    NotGguf,
    /// A GGUF version this parser does not implement.
    Version(u32),
    /// The file ends in the middle of something.
    Truncated(&'static str),
    /// A metadata value of a type the format does not define.
    BadType(u32),
    /// A tensor whose quantisation this crate cannot read.
    UnsupportedQuant(u32),
    /// The model does not carry something the architecture needs.
    Missing(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::NotGguf => write!(f, "not a GGUF file: wrong magic"),
            Self::Version(v) => write!(f, "GGUF version {v} is not one this reads"),
            Self::Truncated(what) => write!(f, "the file ends inside its {what}"),
            Self::BadType(t) => write!(f, "metadata value type {t} is not defined"),
            Self::UnsupportedQuant(t) => write!(f, "tensor type {t} is not one this dequantises"),
            Self::Missing(key) => write!(f, "the model does not carry {key}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A metadata value.
///
/// The format's numeric types are collapsed to two — everything integral to
/// `U64` and everything real to `F64` — because every consumer here wants a
/// count or a constant, and preserving the exact width of `llama.block_count`
/// only creates a place to get the match arm wrong.
#[derive(Debug, Clone)]
pub enum Value {
    U64(u64),
    I64(i64),
    F64(f64),
    Bool(bool),
    Str(String),
    /// A homogeneous array. Strings are kept as strings; everything else is
    /// widened as above.
    Strings(Vec<String>),
    Numbers(Vec<f64>),
}

impl Value {
    /// This value as an unsigned count, if it is one.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(v) => Some(*v),
            Self::I64(v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// This value as a real number, if it is one.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(v) => Some(*v),
            Self::U64(v) => Some(*v as f64),
            Self::I64(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// This value as a string, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// This value as a list of strings, if it is one.
    pub fn as_strings(&self) -> Option<&[String]> {
        match self {
            Self::Strings(v) => Some(v),
            _ => None,
        }
    }
}

/// How a tensor's numbers are stored.
///
/// Only the three this crate can actually read. An unknown one is an error at
/// load rather than a wrong number at inference — a quantisation misread as
/// another of the same block size produces plausible garbage, which is the
/// worst failure a numerical program has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quant {
    /// 32-bit floats, as they are.
    F32,
    /// 16-bit floats.
    F16,
    /// Thirty-two 8-bit integers sharing one 16-bit scale, in 34 bytes.
    Q8_0,
}

impl Quant {
    fn from_u32(t: u32) -> Result<Self, Error> {
        match t {
            0 => Ok(Self::F32),
            1 => Ok(Self::F16),
            8 => Ok(Self::Q8_0),
            other => Err(Error::UnsupportedQuant(other)),
        }
    }

    /// Elements per stored block, and bytes per block.
    ///
    /// One element per "block" for the unquantised kinds, which lets the size
    /// arithmetic below be written once rather than per kind.
    const fn block(self) -> (usize, usize) {
        match self {
            Self::F32 => (1, 4),
            Self::F16 => (1, 2),
            Self::Q8_0 => (32, 34),
        }
    }

    /// Bytes needed for `elements` of this kind.
    pub const fn size_of(self, elements: usize) -> usize {
        let (per_block, bytes) = self.block();
        elements / per_block * bytes
    }
}

/// One tensor: where it is, what shape it is, and how it is stored.
#[derive(Debug, Clone)]
pub struct TensorInfo {
    pub name: String,
    /// Dimensions, fastest-varying first — so `[2048, 8192]` is 8,192 rows of
    /// 2,048, which is a matrix taking a 2,048-vector to an 8,192-vector.
    pub dims: Vec<u64>,
    pub quant: Quant,
    /// Offset from the start of the data section, not from the start of the
    /// file.
    pub offset: u64,
}

impl TensorInfo {
    /// Total elements.
    pub fn elements(&self) -> usize {
        self.dims.iter().product::<u64>() as usize
    }

    /// The length of one row: the fastest-varying dimension.
    pub fn row(&self) -> usize {
        self.dims.first().copied().unwrap_or(0) as usize
    }

    /// How many rows there are.
    pub fn rows(&self) -> usize {
        if self.row() == 0 {
            0
        } else {
            self.elements() / self.row()
        }
    }
}

/// A cursor over the mapped file.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], Error> {
        if self.at + n > self.bytes.len() {
            return Err(Error::Truncated(what));
        }
        let out = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            self.take(4, "header")?.try_into().expect("4 bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(
            self.take(8, "header")?.try_into().expect("8 bytes"),
        ))
    }

    fn string(&mut self) -> Result<String, Error> {
        let len = self.u64()? as usize;
        let bytes = self.take(len, "a string")?;
        // Lossy rather than strict. A vocabulary is byte-level: many of its
        // 128,256 entries are fragments of multi-byte characters and are not
        // valid UTF-8 on their own. Refusing the file over that would refuse
        // every byte-level model there is.
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// One metadata value of the given type tag.
    fn value(&mut self, tag: u32) -> Result<Value, Error> {
        Ok(match tag {
            0 => Value::U64(u64::from(self.take(1, "a u8")?[0])),
            1 => Value::I64(i64::from(self.take(1, "an i8")?[0] as i8)),
            2 => Value::U64(u64::from(u16::from_le_bytes(
                self.take(2, "a u16")?.try_into().expect("2"),
            ))),
            3 => Value::I64(i64::from(i16::from_le_bytes(
                self.take(2, "an i16")?.try_into().expect("2"),
            ))),
            4 => Value::U64(u64::from(self.u32()?)),
            5 => Value::I64(i64::from(self.u32()? as i32)),
            6 => Value::F64(f64::from(f32::from_bits(self.u32()?))),
            7 => Value::Bool(self.take(1, "a bool")?[0] != 0),
            8 => Value::Str(self.string()?),
            9 => {
                let element = self.u32()?;
                let count = self.u64()? as usize;
                if element == 8 {
                    let mut out = Vec::with_capacity(count);
                    for _ in 0..count {
                        out.push(self.string()?);
                    }
                    Value::Strings(out)
                } else {
                    let mut out = Vec::with_capacity(count);
                    for _ in 0..count {
                        out.push(
                            self.value(element)?
                                .as_f64()
                                .ok_or(Error::BadType(element))?,
                        );
                    }
                    Value::Numbers(out)
                }
            }
            10 => Value::U64(self.u64()?),
            11 => Value::I64(self.u64()? as i64),
            12 => Value::F64(f64::from_bits(self.u64()?)),
            other => return Err(Error::BadType(other)),
        })
    }
}

/// A model file, mapped.
pub struct Gguf {
    map: Mmap,
    /// Where the tensor data begins, in file coordinates.
    data_at: usize,
    pub metadata: BTreeMap<String, Value>,
    pub tensors: BTreeMap<String, TensorInfo>,
}

impl Gguf {
    /// Map `path` and read its header.
    ///
    /// The tensor data is not touched here — the pages behind it are faulted in
    /// by whatever reads them, which is what makes a model that is larger than
    /// the working set cost only the working set.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let file = File::open(path.as_ref())?;
        // SAFETY: the file is opened read-only and the map is never written
        // through. A model file being changed underneath a running process
        // would be a problem here, as it is for every mapped file; nothing in
        // this repository writes one.
        let map = unsafe { Mmap::map(&file)? };

        let mut cursor = Cursor { bytes: &map, at: 0 };
        if cursor.take(4, "magic")? != b"GGUF" {
            return Err(Error::NotGguf);
        }
        let version = cursor.u32()?;
        if version != 3 {
            return Err(Error::Version(version));
        }
        let tensor_count = cursor.u64()? as usize;
        let kv_count = cursor.u64()? as usize;

        let mut metadata = BTreeMap::new();
        for _ in 0..kv_count {
            let key = cursor.string()?;
            let tag = cursor.u32()?;
            metadata.insert(key, cursor.value(tag)?);
        }

        let mut tensors = BTreeMap::new();
        for _ in 0..tensor_count {
            let name = cursor.string()?;
            let rank = cursor.u32()? as usize;
            let mut dims = Vec::with_capacity(rank);
            for _ in 0..rank {
                dims.push(cursor.u64()?);
            }
            let quant = Quant::from_u32(cursor.u32()?)?;
            let offset = cursor.u64()?;
            tensors.insert(
                name.clone(),
                TensorInfo {
                    name,
                    dims,
                    quant,
                    offset,
                },
            );
        }

        // The data section is aligned, and the alignment is itself metadata
        // with a documented default. Getting this wrong reads every tensor a
        // few bytes late, which produces a model that loads and answers
        // nonsense.
        let alignment = metadata
            .get("general.alignment")
            .and_then(Value::as_u64)
            .unwrap_or(32) as usize;
        let data_at = cursor.at.div_ceil(alignment) * alignment;

        Ok(Self {
            map,
            data_at,
            metadata,
            tensors,
        })
    }

    /// A metadata value by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// A metadata count, or an error naming the key that was missing.
    pub fn count(&self, key: &str) -> Result<usize, Error> {
        self.get(key)
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .ok_or_else(|| Error::Missing(key.to_string()))
    }

    /// A metadata constant, or an error naming the key that was missing.
    pub fn real(&self, key: &str) -> Result<f32, Error> {
        self.get(key)
            .and_then(Value::as_f64)
            .map(|v| v as f32)
            .ok_or_else(|| Error::Missing(key.to_string()))
    }

    /// A tensor's descriptor, or an error naming it.
    pub fn tensor(&self, name: &str) -> Result<&TensorInfo, Error> {
        self.tensors
            .get(name)
            .ok_or_else(|| Error::Missing(name.to_string()))
    }

    /// The raw bytes of a tensor, inside the mapping.
    pub fn bytes(&self, info: &TensorInfo) -> Result<&[u8], Error> {
        let start = self.data_at + info.offset as usize;
        let len = info.quant.size_of(info.elements());
        self.map
            .get(start..start + len)
            .ok_or(Error::Truncated("tensor data"))
    }

    /// How large the mapping is, which is how large the model is.
    pub fn mapped_bytes(&self) -> usize {
        self.map.len()
    }
}
