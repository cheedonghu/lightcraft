//! The same selected display projection on a worker-owned streaming reader.
use super::super::*;
use std::io::Read;
pub(super) fn read<R: Read>(mut source: R) -> Result<(Value, R)> {
    let mut fields = super::scan::Fields::default();
    let mut bytes = [0; 64 * 1024];
    loop {
        let count = source.read(&mut bytes).map_err(error)?;
        if count == 0 {
            break;
        }
        fields.push(&bytes[..count])?;
    }
    Ok((fields.finish()?, source))
}
