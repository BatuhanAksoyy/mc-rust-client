use super::JoinError;
use bytes::Bytes;
use mc_protocol::configuration;
use std::collections::{BTreeMap, BTreeSet};

/// An entry whose position in its registry determines its numeric network ID.
#[derive(Debug)]
pub struct RegistryEntry {
    /// Canonical namespaced identifier.
    pub id: String,
    /// Validated compound NBT, sharing its received packet storage.
    pub data: Bytes,
}

/// Synchronized registries retained for subsequent chunk/world decoding.
#[derive(Debug, Default)]
pub struct Registries {
    entries: BTreeMap<String, Vec<RegistryEntry>>,
    count: usize,
}

impl Registries {
    /// Look up a registry by canonical identifier; entries remain in wire order.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&[RegistryEntry]> {
        self.entries.get(id).map(Vec::as_slice)
    }

    /// Number of synchronized registries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no registry has been received.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total numeric entries across registries.
    #[must_use]
    pub const fn entry_count(&self) -> usize {
        self.count
    }

    pub(super) fn insert(
        &mut self,
        id: &str,
        entries: Vec<configuration::Entry<'_>>,
        payload: &Bytes,
    ) -> Result<(), JoinError> {
        let id = canonical(id);
        if self.entries.contains_key(&id) {
            return Err(JoinError::InvalidState("duplicate registry"));
        }
        if self.entries.len() >= 128 || entries.len() > configuration::MAX_ENTRIES - self.count {
            return Err(JoinError::Limit);
        }
        let mut names = BTreeSet::new();
        let mut values = Vec::new();
        for entry in entries {
            let id = canonical(entry.id);
            if !names.insert(id.clone()) {
                return Err(JoinError::InvalidState("duplicate registry entry"));
            }
            let data = entry
                .data
                .ok_or(JoinError::InvalidState("registry data omitted without known packs"))?;
            values.push(RegistryEntry { id, data: payload.slice_ref(data) });
        }
        self.count += values.len();
        self.entries.insert(id, values);
        Ok(())
    }
}

fn canonical(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}
