//! Selected display fields have a read/allocation budget; skipped source text does not.
use super::super::*;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::cell::Cell;
use std::io::Read;
use std::rc::Rc;

struct BudgetRead<R> {
    inner: R,
    budget: Rc<Cell<Option<usize>>>,
}
impl<R: Read> Read for BudgetRead<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let limit = self.budget.get().unwrap_or(bytes.len()).min(bytes.len());
        if limit == 0 {
            return Err(std::io::Error::other("round display field exceeds bounded read budget"));
        }
        let n = self.inner.read(bytes.get_mut(..limit).ok_or_else(|| std::io::Error::other("read bounds"))?)?;
        if let Some(left) = self.budget.get() {
            self.budget.set(Some(left.saturating_sub(n)));
        }
        Ok(n)
    }
}
#[derive(Clone)]
struct Fields {
    budget: Rc<Cell<Option<usize>>>,
    remaining: Rc<Cell<usize>>,
    stage: bool,
}
impl<'de> DeserializeSeed<'de> for Fields {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Fields {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("recorded execution display fields")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<Value, M::Error> {
        let mut out = serde_json::Map::new();
        loop {
            self.budget.set(Some(256));
            let Some(key) = map.next_key::<String>()? else { break };
            if key.len() > 128 {
                return Err(serde::de::Error::custom("record field name too long"));
            }
            if !self.stage && key == "stages" {
                self.budget.set(Some(256));
                out.insert(key, map.next_value_seed(Stages(self.clone()))?);
                continue;
            }
            let allowed = if self.stage {
                matches!(
                    key.as_str(),
                    "stage"
                        | "plan_id"
                        | "reason"
                        | "execution_outcome"
                        | "delivery_state"
                        | "locally_published"
                        | "preview_message_id"
                        | "display_before"
                        | "display_target"
                        | "display_actual"
                )
            } else {
                matches!(key.as_str(), "local_round_id" | "photo_id" | "local_terminal" | "before_preview_message_id" | "recent_complete_hash")
            };
            if allowed {
                self.budget.set(Some(self.remaining.get()));
                let value: Value = map.next_value()?;
                let length = super::serialized_length(&value).map_err(serde::de::Error::custom)?;
                let remaining =
                    self.remaining.get().checked_sub(length).ok_or_else(|| serde::de::Error::custom("round display exceeds view budget"))?;
                self.remaining.set(remaining);
                out.insert(key, value);
            } else {
                self.budget.set(None);
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
        Ok(Value::Object(out))
    }
}
struct Stages(Fields);
impl<'de> DeserializeSeed<'de> for Stages {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Stages {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("recorded execution stages")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> std::result::Result<Value, S::Error> {
        let mut out = Vec::new();
        loop {
            self.0.budget.set(Some(256));
            let mut fields = self.0.clone();
            fields.stage = true;
            let Some(value) = seq.next_element_seed(fields)? else { break };
            if out.len() >= 64 {
                return Err(serde::de::Error::custom("invalid recorded execution stage count"));
            }
            out.push(value);
        }
        Ok(Value::Array(out))
    }
}
pub(super) fn read<R: Read>(source: R) -> Result<(Value, R)> {
    let budget = Rc::new(Cell::new(Some(256)));
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, BudgetRead { inner: source, budget: budget.clone() });
    let mut decoder = serde_json::Deserializer::from_reader(&mut reader);
    let value =
        Fields { budget: budget.clone(), remaining: Rc::new(Cell::new(1024 * 1024)), stage: false }.deserialize(&mut decoder).map_err(error)?;
    budget.set(Some(256));
    decoder.end().map_err(error)?;
    Ok((value, reader.into_inner().inner))
}
