// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// Bounded mod-owned fuel definitions. Names travel instead of runtime registry IDs.
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const CHUNK_BYTES: usize = 24_000;
const MAX_ENTRIES: usize = 8192;
const MAX_ID: usize = 256;
const HEADER: usize = 29;
const VERSION: u8 = 1;
const MAGIC: &[u8; 4] = b"CBDS";
pub(crate) const REQUEST: &[u8] = b"CBRQ\x01";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub special: BTreeMap<String, i32>,
    pub regular: BTreeMap<String, i32>,
    pub ordinary: BTreeMap<String, i32>,
    pub special_tag: BTreeSet<String>,
    pub regular_tag: BTreeSet<String>,
    pub capturable: BTreeSet<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Generation {
    pub world: u64,
    pub reload: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Limit,
    Format,
    Duplicate,
    Version,
}

pub(crate) fn valid_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    id.len() <= MAX_ID
        && !namespace.is_empty()
        && !path.is_empty()
        && namespace
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
        && path
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"/_.-".contains(&c))
}
impl Snapshot {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, Error> {
        let maps = [&self.special, &self.regular, &self.ordinary];
        let sets = [&self.special_tag, &self.regular_tag, &self.capturable];
        let mut size = 24usize;
        for map in maps {
            if map.len() > MAX_ENTRIES {
                return Err(Error::Limit);
            }
            for (id, time) in map {
                if !valid_id(id) || *time <= 0 {
                    return Err(Error::Format);
                }
                size = size.checked_add(6 + id.len()).ok_or(Error::Limit)?;
            }
        }
        for set in sets {
            if set.len() > MAX_ENTRIES {
                return Err(Error::Limit);
            }
            for id in set {
                if !valid_id(id) {
                    return Err(Error::Format);
                }
                size = size.checked_add(2 + id.len()).ok_or(Error::Limit)?;
            }
        }
        if size > MAX_BYTES {
            return Err(Error::Limit);
        }
        let mut out = Vec::with_capacity(size);
        for map in maps {
            out.extend_from_slice(&(map.len() as u32).to_be_bytes());
            for (id, time) in map {
                put_id(&mut out, id);
                out.extend_from_slice(&time.to_be_bytes());
            }
        }
        for set in sets {
            out.extend_from_slice(&(set.len() as u32).to_be_bytes());
            for id in set {
                put_id(&mut out, id);
            }
        }
        Ok(out)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let mut input = Input(bytes);
        let mut snapshot = Self::default();
        for map in [
            &mut snapshot.special,
            &mut snapshot.regular,
            &mut snapshot.ordinary,
        ] {
            let count = input.count()?;
            for _ in 0..count {
                let id = input.id()?;
                let time = i32::from_be_bytes(input.take(4)?.try_into().unwrap());
                if time <= 0 {
                    return Err(Error::Format);
                }
                if map.insert(id, time).is_some() {
                    return Err(Error::Duplicate);
                }
            }
        }
        for set in [
            &mut snapshot.special_tag,
            &mut snapshot.regular_tag,
            &mut snapshot.capturable,
        ] {
            let count = input.count()?;
            for _ in 0..count {
                if !set.insert(input.id()?) {
                    return Err(Error::Duplicate);
                }
            }
        }
        if !input.0.is_empty() {
            return Err(Error::Format);
        }
        Ok(snapshot)
    }
}
fn put_id(out: &mut Vec<u8>, id: &str) {
    out.extend_from_slice(&(id.len() as u16).to_be_bytes());
    out.extend_from_slice(id.as_bytes());
}
struct Input<'a>(&'a [u8]);
impl<'a> Input<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        let (value, rest) = self.0.split_at_checked(size).ok_or(Error::Format)?;
        self.0 = rest;
        Ok(value)
    }
    fn count(&mut self) -> Result<usize, Error> {
        let count = u32::from_be_bytes(self.take(4)?.try_into().unwrap()) as usize;
        if count > MAX_ENTRIES {
            return Err(Error::Limit);
        }
        Ok(count)
    }
    fn id(&mut self) -> Result<String, Error> {
        let size = u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as usize;
        if size > MAX_ID {
            return Err(Error::Limit);
        }
        let id = std::str::from_utf8(self.take(size)?).map_err(|_| Error::Format)?;
        if !valid_id(id) {
            return Err(Error::Format);
        }
        Ok(id.to_owned())
    }
}

pub(crate) fn frames(key: Generation, snapshot: &Snapshot) -> Result<Vec<Vec<u8>>, Error> {
    if key.world == 0 || key.reload == 0 {
        return Err(Error::Format);
    }
    let body = snapshot.encode()?;
    let parts = body.len().div_ceil(CHUNK_BYTES);
    let mut frames = Vec::with_capacity(parts);
    for (index, data) in body.chunks(CHUNK_BYTES).enumerate() {
        let mut out = Vec::with_capacity(HEADER + data.len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&key.world.to_be_bytes());
        out.extend_from_slice(&key.reload.to_be_bytes());
        out.extend_from_slice(&(index as u16).to_be_bytes());
        out.extend_from_slice(&(parts as u16).to_be_bytes());
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        frames.push(out);
    }
    Ok(frames)
}
struct Pending {
    key: Generation,
    size: usize,
    parts: Vec<Option<Vec<u8>>>,
}
#[derive(Default)]
pub(crate) struct Receiver {
    pub active: Option<(Generation, Snapshot)>,
    pending: Option<Pending>,
}
impl Receiver {
    pub(crate) fn receive(&mut self, bytes: &[u8]) -> Result<bool, Error> {
        if bytes.len() > HEADER + CHUNK_BYTES {
            return Err(Error::Limit);
        }
        let mut input = Input(bytes);
        if input.take(4)? != MAGIC {
            return Err(Error::Format);
        }
        if input.take(1)?[0] != VERSION {
            return Err(Error::Version);
        }
        let key = Generation {
            world: u64::from_be_bytes(input.take(8)?.try_into().unwrap()),
            reload: u64::from_be_bytes(input.take(8)?.try_into().unwrap()),
        };
        let index = u16::from_be_bytes(input.take(2)?.try_into().unwrap()) as usize;
        let count = u16::from_be_bytes(input.take(2)?.try_into().unwrap()) as usize;
        let size = u32::from_be_bytes(input.take(4)?.try_into().unwrap()) as usize;
        if key.world == 0
            || key.reload == 0
            || !(24..=MAX_BYTES).contains(&size)
            || count != size.div_ceil(CHUNK_BYTES)
            || index >= count
            || input.0.len() != (size - index * CHUNK_BYTES).min(CHUNK_BYTES)
        {
            return Err(Error::Format);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|(active, _)| key <= *active)
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| key < pending.key)
        {
            return Ok(false);
        }
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.key != key)
        {
            self.pending = Some(Pending {
                key,
                size,
                parts: vec![None; count],
            });
        }
        let pending = self.pending.as_mut().unwrap();
        if pending.size != size || pending.parts.len() != count {
            self.pending = None;
            return Err(Error::Format);
        }
        let part = &mut pending.parts[index];
        if let Some(old) = part {
            if old != input.0 {
                self.pending = None;
                return Err(Error::Duplicate);
            }
            return Ok(false);
        }
        *part = Some(input.0.to_vec());
        if pending.parts.iter().any(Option::is_none) {
            return Ok(false);
        }
        let mut body = Vec::with_capacity(size);
        for part in &mut pending.parts {
            body.extend(part.take().unwrap());
        }
        self.pending = None;
        let snapshot = Snapshot::decode(&body)?;
        self.active = Some((key, snapshot));
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            special: [("create:blaze_cake".into(), 3200)].into(),
            ordinary: [("minecraft:coal".into(), 1600)].into(),
            special_tag: ["demo:hot".into()].into(),
            capturable: ["minecraft:blaze".into()].into(),
            ..Default::default()
        }
    }
    fn key(reload: u64) -> Generation {
        Generation { world: 1, reload }
    }
    #[test]
    fn snapshot_complete_commit_reload_and_old_generation() {
        let mut receiver = Receiver::default();
        let initial = snapshot();
        for frame in frames(key(1), &initial).unwrap() {
            receiver.receive(&frame).unwrap();
        }
        let mut updated = initial.clone();
        for i in 0..4000 {
            updated.regular.insert(format!("demo:fuel_{i}"), 123);
        }
        updated.special.clear();
        updated.capturable.clear();
        let packet = frames(key(2), &updated).unwrap();
        assert!(packet.len() > 1);
        for frame in packet.iter().skip(1).rev() {
            assert!(!receiver.receive(frame).unwrap());
            assert_eq!(receiver.active.as_ref().unwrap().1, initial);
        }
        assert!(receiver.receive(&packet[0]).unwrap());
        assert_eq!(receiver.active.as_ref().unwrap().1, updated);
        assert!(
            !receiver
                .receive(&frames(key(1), &initial).unwrap()[0])
                .unwrap()
        );
        assert_eq!(receiver.active.as_ref().unwrap().1, updated);
    }
    #[test]
    fn malformed_and_partial_never_replace_active() {
        let initial = snapshot();
        let good = frames(key(1), &initial).unwrap().remove(0);
        let mut receiver = Receiver::default();
        receiver.receive(&good).unwrap();
        let original = receiver.active.clone();
        let newer = frames(key(2), &initial).unwrap().remove(0);
        for n in 0..newer.len() {
            assert!(receiver.receive(&newer[..n]).is_err());
            assert_eq!(receiver.active, original);
        }
        let mut wrong = newer.clone();
        wrong[4] = 2;
        assert_eq!(receiver.receive(&wrong), Err(Error::Version));
        for offset in [21, 23, 25, HEADER] {
            let mut wrong = newer.clone();
            wrong[offset] = 255;
            let _ = receiver.receive(&wrong);
            assert_eq!(receiver.active, original);
        }
        assert_eq!(Snapshot::decode(&[0; 23]), Err(Error::Format));
        let mut trailing = initial.encode().unwrap();
        trailing.push(0);
        assert_eq!(Snapshot::decode(&trailing), Err(Error::Format));
    }
    #[test]
    fn duplicate_entries_invalid_ids_and_limits_are_rejected() {
        let mut body = Vec::new();
        body.extend(2u32.to_be_bytes());
        for _ in 0..2 {
            put_id(&mut body, "demo:fuel");
            body.extend(1i32.to_be_bytes());
        }
        body.extend([0; 20]);
        assert_eq!(Snapshot::decode(&body), Err(Error::Duplicate));
        for id in ["bare", "a:", ":a", "a:b:c", "a:A", "a:hello world"] {
            let mut value = snapshot();
            value.special.insert(id.into(), 1);
            assert_eq!(value.encode(), Err(Error::Format));
        }
        let mut value = snapshot();
        value.special.insert("demo:bad".into(), 0);
        assert_eq!(value.encode(), Err(Error::Format));
        let huge = [255, 255, 255, 255];
        assert_eq!(Snapshot::decode(&huge), Err(Error::Limit));
        assert_eq!(
            Receiver::default().receive(&vec![0; HEADER + CHUNK_BYTES + 1]),
            Err(Error::Limit)
        );
    }
}
