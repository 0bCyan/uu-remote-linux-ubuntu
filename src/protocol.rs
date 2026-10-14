//! Dependency-free protobuf wire inspection for the official binary control channel.
//! This decodes fields only. It intentionally assigns no heartbeat/input semantics
//! and sends no acknowledgements: those require evidence from the application
//! callback above libstreamer::ControlledControlDataHandler::OnReceiveData.

#[derive(Debug, PartialEq, Eq)]
pub enum WireValue<'a> {
    Varint(u64),
    Fixed64([u8; 8]),
    Bytes(&'a [u8]),
    Fixed32([u8; 4]),
}

#[derive(Debug, PartialEq, Eq)]
pub struct WireField<'a> {
    pub number: u32,
    pub value: WireValue<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    VarintOverflow,
    InvalidField,
    UnsupportedWireType(u8),
}

fn varint(input: &[u8], offset: &mut usize) -> Result<u64, WireError> {
    let mut result = 0;
    for shift in (0..70).step_by(7) {
        let byte = *input.get(*offset).ok_or(WireError::Truncated)?;
        *offset += 1;
        if shift == 63 && byte > 1 {
            return Err(WireError::VarintOverflow);
        }
        result |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(result);
        }
    }
    Err(WireError::VarintOverflow)
}

fn take<'a>(input: &'a [u8], offset: &mut usize, length: usize) -> Result<&'a [u8], WireError> {
    let end = offset.checked_add(length).ok_or(WireError::Truncated)?;
    let value = input.get(*offset..end).ok_or(WireError::Truncated)?;
    *offset = end;
    Ok(value)
}

/// Inspect a complete protobuf message, preserving opaque nested payloads.
/// Deprecated groups are rejected. No payload contents should be logged.
pub fn decode_wire(input: &[u8]) -> Result<Vec<WireField<'_>>, WireError> {
    let mut offset = 0;
    let mut fields = Vec::new();
    while offset < input.len() {
        let key = varint(input, &mut offset)?;
        let number = key >> 3;
        if number == 0 || number > 0x1fff_ffff {
            return Err(WireError::InvalidField);
        }
        let value = match (key & 7) as u8 {
            0 => WireValue::Varint(varint(input, &mut offset)?),
            1 => WireValue::Fixed64(take(input, &mut offset, 8)?.try_into().unwrap()),
            2 => {
                let length = usize::try_from(varint(input, &mut offset)?)
                    .map_err(|_| WireError::Truncated)?;
                WireValue::Bytes(take(input, &mut offset, length)?)
            }
            5 => WireValue::Fixed32(take(input, &mut offset, 4)?.try_into().unwrap()),
            wire => return Err(WireError::UnsupportedWireType(wire)),
        };
        fields.push(WireField { number: number as u32, value });
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_mixed_unknown_fields_and_nested_bytes() {
        let bytes = [8, 150, 1, 18, 2, 8, 3, 29, 1, 2, 3, 4];
        assert_eq!(decode_wire(&bytes), Ok(vec![
            WireField { number: 1, value: WireValue::Varint(150) },
            WireField { number: 2, value: WireValue::Bytes(&[8, 3]) },
            WireField { number: 3, value: WireValue::Fixed32([1, 2, 3, 4]) },
        ]));
    }

    #[test]
    fn rejects_truncation_and_u64_overflow() {
        assert_eq!(decode_wire(&[18, 4, 1]), Err(WireError::Truncated));
        assert_eq!(decode_wire(&[8, 128]), Err(WireError::Truncated));
        let mut bytes = vec![8];
        bytes.extend_from_slice(&[255; 9]);
        bytes.push(2);
        assert_eq!(decode_wire(&bytes), Err(WireError::VarintOverflow));
    }

    #[test]
    fn rejects_invalid_fields_and_groups() {
        assert_eq!(decode_wire(&[0]), Err(WireError::InvalidField));
        assert_eq!(decode_wire(&[11]), Err(WireError::UnsupportedWireType(3)));
        assert_eq!(decode_wire(&[255, 255, 255, 255, 31]), Err(WireError::InvalidField));
    }
}
