use base64::Engine;

use super::{publickey, PrimitiveType, Struct, Type};

type MemoryReader<'a> = dyn Fn(u64) -> Option<[u64; 4]> + 'a;

pub trait TypeReader {
    fn read(&self, reader: &MemoryReader, addr: u64) -> Result<Value, Box<dyn std::error::Error>>;
}

#[derive(Debug)]
pub enum Value {
    Nullable(Option<Box<Value>>),
    Boolean(bool),
    UInt32(u32),
    UInt64(u64),
    Float32(f32),
    Hash([u64; 4]),
    Int32(i32),
    String(String),
    Bytes(Vec<u8>),
    CollectionReference(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    PublicKey(publickey::Key),
    StructValue(Vec<(String, Value)>),
}

impl TypeReader for PrimitiveType {
    fn read(&self, reader: &MemoryReader, addr: u64) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(match self {
            PrimitiveType::Boolean => {
                let [b, _, _, _] = reader(addr).ok_or("invalid address for boolean")?;
                assert!(b == 0 || b == 1);
                Value::Boolean(b != 0)
            }
            PrimitiveType::UInt32 => {
                let [x, _, _, _] = reader(addr).ok_or("invalid address for uint32")?;
                Value::UInt32(u32::try_from(x).unwrap())
            }
            PrimitiveType::Int32 => {
                let [x, _, _, _] = reader(addr).ok_or("invalid address for int32")?;
                Value::Int32(i32::try_from(x).unwrap())
            }
            PrimitiveType::UInt64 => {
                let [high, low, _, _] = reader(addr).ok_or("invalid address for uint64")?;
                Value::UInt64((high << 32) | low)
            }
            PrimitiveType::Float32 => {
                let [bits, _, _, _] = reader(addr).ok_or("invalid address for float32")?;
                Value::Float32(f32::from_bits(bits as u32))
            }
        })
    }
}

impl TypeReader for Struct {
    fn read(&self, reader: &MemoryReader, addr: u64) -> Result<Value, Box<dyn std::error::Error>> {
        let mut fields = Vec::new();
        let mut current_addr = addr;
        for (name, type_) in &self.fields {
            let value = type_.read(reader, current_addr)?;
            fields.push((name.clone(), value));
            current_addr += type_.miden_width() as u64;
        }
        Ok(Value::StructValue(fields))
    }
}

impl TypeReader for Type {
    fn read(&self, reader: &MemoryReader, addr: u64) -> Result<Value, Box<dyn std::error::Error>> {
        match self {
            Type::Nullable(t) => {
                let [is_null, _, _, _] = reader(addr).ok_or("invalid address for nullable")?;
                if is_null == 0 {
                    Ok(Value::Nullable(None))
                } else {
                    Ok(Value::Nullable(Some(Box::new(t.read(reader, addr + 1)?))))
                }
            }
            Type::PrimitiveType(pt) => pt.read(reader, addr),
            Type::Struct(s) => s.read(reader, addr),
            Type::Hash => Ok(reader(addr)
                .ok_or("invalid address for hash")
                .map(Value::Hash)?),
            Type::String => {
                let mut bytes = vec![];

                let length = reader(addr).ok_or("invalid address for string length")?[0];
                let data_ptr = reader(addr + 1).ok_or("invalid address for string data ptr")?[0];
                for i in 0..length {
                    let byte = reader(data_ptr + i).ok_or("invalid address for string byte")?[0];
                    bytes.push(byte as u8);
                }

                let string = String::from_utf8(bytes)?;

                Ok(Value::String(string))
            }
            Type::Bytes => {
                let mut bytes = vec![];

                let length = reader(addr).ok_or("invalid address for bytes length")?[0];
                let data_ptr = reader(addr + 1).ok_or("invalid address for bytes data ptr")?[0];
                for i in 0..length {
                    let byte = reader(data_ptr + i).ok_or("invalid address for bytes byte")?[0];
                    bytes.push(byte as u8);
                }

                Ok(Value::Bytes(bytes))
            }
            Type::CollectionReference { .. } => {
                let mut bytes = vec![];

                let length =
                    reader(addr).ok_or("invalid address for collection reference length")?[0];
                let data_ptr =
                    reader(addr + 1).ok_or("invalid address for collection reference data ptr")?[0];
                for i in 0..length {
                    let byte = reader(data_ptr + i)
                        .ok_or("invalid address for collection reference byte")?[0];
                    bytes.push(byte as u8);
                }

                Ok(Value::CollectionReference(bytes))
            }
            Type::Array(t) => {
                let mut values = vec![];

                let length = reader(addr + 1).ok_or("invalid address for array length")?[0];
                let data_ptr = reader(addr + 2).ok_or("invalid address for array data ptr")?[0];
                for i in 0..length {
                    let value = t.read(reader, data_ptr + i * t.miden_width() as u64)?;
                    values.push(value);
                }

                Ok(Value::Array(values))
            }
            Type::Map(k, v) => {
                let mut key_values = Vec::new();

                let key_array_data_start_ptr = reader(addr + 2).unwrap()[0];
                let value_array_data_start_ptr =
                    reader(addr + super::array::WIDTH as u64 + 2).unwrap()[0];
                let length = reader(addr + 1).ok_or("invalid address for map keys length")?[0];

                for i in 0..length {
                    let key = k.read(
                        reader,
                        key_array_data_start_ptr + i * k.miden_width() as u64,
                    )?;
                    let value = v.read(
                        reader,
                        value_array_data_start_ptr + i * v.miden_width() as u64,
                    )?;

                    key_values.push((key, value));
                }

                Ok(Value::Map(key_values))
            }
            Type::PublicKey => {
                let kty = reader(addr)
                    .map(|x| x[0])
                    .ok_or("invalid address for public key kty")?;
                let crv = reader(addr + 1)
                    .map(|x| x[0])
                    .ok_or("invalid address for public key crv")?;
                let alg = reader(addr + 2)
                    .map(|x| x[0])
                    .ok_or("invalid address for public key alg")?;
                let use_ = reader(addr + 3)
                    .map(|x| x[0])
                    .ok_or("invalid address for public key use")?;
                let extra_ptr = reader(addr + 4)
                    .map(|x| x[0])
                    .ok_or("invalid address for public key extra ptr")?;

                let mut extra_bytes = vec![];
                for i in 0..64 {
                    let byte = reader(extra_ptr + i)
                        .map(|x| x[0])
                        .ok_or("invalid address for public key extra byte")?;
                    extra_bytes.push(byte as u8);
                }

                let x = extra_bytes[0..32].try_into()?;
                let y = extra_bytes[32..64].try_into()?;

                let key = publickey::Key {
                    kty: (kty as u8).into(),
                    crv: (crv as u8).into(),
                    alg: (alg as u8).into(),
                    use_: (use_ as u8).into(),
                    x,
                    y,
                };

                Ok(Value::PublicKey(key))
            }
        }
    }
}

pub trait Parser {
    fn parse(&self, value: &str) -> Result<Value, Box<dyn std::error::Error>>;
}

impl Parser for PrimitiveType {
    fn parse(&self, value: &str) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(match self {
            PrimitiveType::Boolean => Value::Boolean(value.parse()?),
            PrimitiveType::UInt32 => Value::UInt32(value.parse()?),
            PrimitiveType::Int32 => Value::Int32(value.parse()?),
            PrimitiveType::UInt64 => Value::UInt64(value.parse()?),
            PrimitiveType::Float32 => Value::Float32(value.parse()?),
        })
    }
}

impl Parser for Struct {
    fn parse(&self, value: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let mut fields = Vec::new();
        let mut value = value;
        for (name, type_) in &self.fields {
            let (field_value, rest) = value.split_once(',').ok_or("invalid value")?;
            fields.push((name.clone(), type_.parse(field_value)?));
            value = rest;
        }
        Ok(Value::StructValue(fields))
    }
}

impl Parser for Type {
    fn parse(&self, value: &str) -> Result<Value, Box<dyn std::error::Error>> {
        match self {
            Type::Nullable(t) => {
                if value == "null" {
                    Ok(Value::Nullable(None))
                } else {
                    Ok(Value::Nullable(Some(Box::new(t.parse(value)?))))
                }
            }
            Type::PrimitiveType(pt) => pt.parse(value),
            Type::Struct(s) => s.parse(value),
            Type::Hash => {
                let mut bytes = vec![];
                if !value.is_empty() {
                    for byte in value.split(',') {
                        bytes.push(byte.parse()?);
                    }
                }
                let mut hash = [0; 4];
                hash.copy_from_slice(&bytes);
                Ok(Value::Hash(hash))
            }
            Type::String => Ok(Value::String(value.to_string())),
            Type::Bytes => {
                let mut bytes = vec![];
                if !value.is_empty() {
                    for byte in value.split(',') {
                        bytes.push(byte.parse()?);
                    }
                }
                Ok(Value::Bytes(bytes))
            }
            Type::CollectionReference { .. } => {
                let mut bytes = vec![];
                if !value.is_empty() {
                    for byte in value.split(',') {
                        bytes.push(byte.parse()?);
                    }
                }
                Ok(Value::CollectionReference(bytes))
            }
            Type::Array(t) => {
                let mut values = vec![];
                if !value.is_empty() {
                    for value in value.split(';') {
                        values.push(t.parse(value)?);
                    }
                }
                Ok(Value::Array(values))
            }
            Type::Map(k, v) => {
                let mut key_values = vec![];
                if !value.is_empty() {
                    let mut parts = value.split(';');
                    loop {
                        let Some(key) = parts.next() else {
                            break;
                        };

                        let value = parts.next().expect("Missing value in map");

                        key_values.push((k.parse(key)?, v.parse(value)?));
                    }
                }
                Ok(Value::Map(key_values))
            }
            Type::PublicKey => {
                let mut values = value.split(',');
                let kty = values.next().ok_or("missing kty")?;
                let crv = values.next().ok_or("missing crv")?;
                let alg = values.next().ok_or("missing alg")?;
                let use_ = values.next().ok_or("missing use")?;
                let x_base64 = values.next().ok_or("missing x")?;
                let y_base64 = values.next().ok_or("missing y")?;

                let x = base64::engine::general_purpose::URL_SAFE.decode(x_base64)?;
                let y = base64::engine::general_purpose::URL_SAFE.decode(y_base64)?;

                let mut extra_bytes = vec![];
                extra_bytes.extend_from_slice(&x);
                extra_bytes.extend_from_slice(&y);

                let key = publickey::Key {
                    kty: kty.parse().map_err(|_| "invalid kty")?,
                    crv: crv.parse().map_err(|_| "invalid crv")?,
                    alg: alg.parse().map_err(|_| "invalid alg")?,
                    use_: use_.parse().map_err(|_| "invalid use")?,
                    x: x.try_into().map_err(|_| "invalid x")?,
                    y: y.try_into().map_err(|_| "invalid y")?,
                };

                Ok(Value::PublicKey(key))
            }
        }
    }
}

impl Value {
    pub fn serialize(&self) -> Vec<u64> {
        match self {
            Value::Nullable(opt) => match opt {
                None => vec![0],
                Some(v) => [1].into_iter().chain(v.serialize().into_iter()).collect(),
            },
            Value::Boolean(b) => vec![*b as u64],
            Value::UInt32(x) => vec![u64::from(*x)],
            Value::UInt64(x) => vec![*x >> 32, *x & 0xffffffff],
            Value::Int32(x) => vec![*x as u64],
            Value::Float32(x) => vec![x.to_bits() as u64],
            Value::Hash(h) => h.to_vec(),
            Value::String(s) => [s.len() as u64]
                .into_iter()
                .chain(s.bytes().map(|b| b as u64))
                .collect(),
            Value::Bytes(b) => [b.len() as u64]
                .into_iter()
                .chain(b.iter().map(|b| *b as u64))
                .collect(),
            Value::Array(values) => [values.len() as u64]
                .into_iter()
                .chain(values.iter().flat_map(|v| v.serialize()))
                .collect(),
            // Map is serialized as [keys_arr..., values_arr...] so that we can reuse read_advice_array
            Value::Map(key_values) => []
                .into_iter()
                .chain([key_values.len() as u64])
                .chain(key_values.iter().flat_map(|(k, _)| k.serialize()))
                .chain([key_values.len() as u64])
                .chain(key_values.iter().flat_map(|(_, v)| v.serialize()))
                .collect(),
            Value::CollectionReference(cr) => [cr.len() as u64]
                .into_iter()
                .chain(cr.iter().map(|b| *b as u64))
                .collect(),
            Value::PublicKey(k) => vec![
                u8::from(k.kty) as u64,
                u8::from(k.crv) as u64,
                u8::from(k.alg) as u64,
                u8::from(k.use_) as u64,
            ]
            .into_iter()
            .chain(k.x.iter().map(|b| *b as u64))
            .chain(k.y.iter().map(|b| *b as u64))
            .collect(),
            Value::StructValue(sv) => sv
                .iter()
                .flat_map(|(_, v)| v.serialize())
                .collect::<Vec<_>>(),
        }
    }
}