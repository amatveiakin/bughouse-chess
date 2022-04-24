// TODO: Replace home-made read/write functions with tokio or another proper alternative.
//   ... or use intermediate format other than String to support binary formats.

use std::io;

use byteorder::{ByteOrder, LittleEndian};
use serde::{de, Serialize};


pub const PORT: u16 = 38617;

pub fn write_str(writer: &mut impl io::Write, data: &str) -> io::Result<()> {
    let mut buf = [0u8; 4];
    LittleEndian::write_u32(&mut buf, data.len() as u32);
    writer.write_all(&buf)?;
    writer.write_all(&data.as_ref())?;
    Ok(())
}

pub fn read_str(reader: &mut impl io::Read) -> io::Result<String> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = LittleEndian::read_u32(&mut len_buf);
    let mut content_buf = vec![0; len.try_into().unwrap()];
    reader.read_exact(&mut content_buf)?;
    Ok(String::from_utf8(content_buf).unwrap())
}

pub fn write_obj(writer: &mut impl io::Write, obj: &impl Serialize) -> io::Result<()> {
    write_str(writer, &serde_json::to_string(obj).unwrap())
}

// TODO: Combine `parse_obj(read_str(...))` into `read_obj(...)`.
// TODO: Make return type deducible.
pub fn parse_obj<'a, T>(s: &'a str) -> Result<T, serde_json::Error>
where
    T: de::Deserialize<'a>,
{
    serde_json::from_str(s)
}
