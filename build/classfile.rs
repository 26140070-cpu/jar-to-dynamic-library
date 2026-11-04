pub struct ClassFile {
    pub name: String,
    pub super_class: Option<String>,
    pub interfaces: Vec<String>,
    pub data: Vec<u8>,
    pub priority: u32,
}

enum Constant {
    Utf8(String),
    Class(u16),
    Other,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| "unexpected end of class file".to_string())?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn decode_modified_utf8(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i] as u16;
        if b & 0x80 == 0 {
            units.push(b);
            i += 1;
        } else if b & 0xE0 == 0xC0 && i + 1 < bytes.len() {
            units.push((b & 0x1F) << 6 | (bytes[i + 1] as u16 & 0x3F));
            i += 2;
        } else if b & 0xF0 == 0xE0 && i + 2 < bytes.len() {
            units.push((b & 0x0F) << 12 | (bytes[i + 1] as u16 & 0x3F) << 6 | (bytes[i + 2] as u16 & 0x3F));
            i += 3;
        } else {
            units.push(0xFFFD);
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

impl ClassFile {
    pub fn parse(data: Vec<u8>) -> Result<ClassFile, String> {
        let mut r = Reader { bytes: &data, pos: 0 };

        if r.u32()? != 0xCAFE_BABE {
            return Err("Not a class file".into());
        }
        r.u16()?;
        r.u16()?;

        let cp_count = r.u16()? as usize;
        let mut cp: Vec<Constant> = Vec::with_capacity(cp_count);
        cp.push(Constant::Other);
        while cp.len() < cp_count {
            let tag = r.u8()?;
            match tag {
                1 => {
                    let len = r.u16()? as usize;
                    cp.push(Constant::Utf8(decode_modified_utf8(r.take(len)?)));
                }
                3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => {
                    r.take(4)?;
                    cp.push(Constant::Other);
                }
                5 | 6 => {
                    r.take(8)?;
                    cp.push(Constant::Other);
                    cp.push(Constant::Other);
                }
                7 => {
                    let name_index = r.u16()?;
                    cp.push(Constant::Class(name_index));
                }
                8 | 16 | 19 | 20 => {
                    r.take(2)?;
                    cp.push(Constant::Other);
                }
                15 => {
                    r.take(3)?;
                    cp.push(Constant::Other);
                }
                other => return Err(format!("Failed to read constant pool because of type {other}")),
            }
        }

        let class_name = |cp: &[Constant], class_index: u16| -> Result<Option<String>, String> {
            if class_index == 0 {
                return Ok(None);
            }
            match cp.get(class_index as usize) {
                Some(Constant::Class(name_index)) => match cp.get(*name_index as usize) {
                    Some(Constant::Utf8(s)) => Ok(Some(s.clone())),
                    _ => Err("class entry does not point to a Utf8 name".into()),
                },
                _ => Err("index does not point to a Class entry".into()),
            }
        };

        r.u16()?;
        let this_index = r.u16()?;
        let super_index = r.u16()?;
        let name = class_name(&cp, this_index)?.ok_or("this_class is 0")?;
        let super_class = class_name(&cp, super_index)?;

        let interface_count = r.u16()? as usize;
        let mut interfaces = Vec::with_capacity(interface_count);
        for _ in 0..interface_count {
            let index = r.u16()?;
            interfaces.push(class_name(&cp, index)?.ok_or("interface index is 0")?);
        }

        Ok(ClassFile {
            name,
            super_class,
            interfaces,
            data,
            priority: 0,
        })
    }
}
