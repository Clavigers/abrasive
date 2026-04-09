//! Per-scope build slots.
//!
//! Each (team, scope) gets `SLOTS_PER_SCOPE` workspace dirs on disk.
//! A user is hashed to a preferred slot index — repeated builds from
//! the same user land in the same slot, so cargo's incremental state
//! stays warm. On contention the user falls back to the first free
//! slot, accepting a colder cache for that build.
//!
//! Slot state is in-memory only; the on-disk dirs persist forever.
//! Acquisition is mediated by a `SlotGuard` whose `Drop` releases the
//! slot — so a panic mid-build still frees the slot for the next user.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::constants::SLOTS_PER_SCOPE;

/// Per-slot cache of "the last fingerprint we successfully synced
/// from a client into this slot." On the next connection from any
/// client, if their probe fingerprint matches the cached value the
/// daemon knows the slot's source tree is already up to date and
/// can skip the manifest/sync phase entirely.
#[derive(Clone, Default)]
pub struct FingerprintCache {
    inner: Arc<Mutex<HashMap<FpKey, [u8; 32]>>>,
}

type FpKey = (usize, String, String);

impl FingerprintCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn matches(&self, slot: &SlotGuard, team: &str, scope: &str, fp: &[u8; 32]) -> bool {
        let key = (slot.index, team.to_string(), scope.to_string());
        self.inner.lock().unwrap().get(&key) == Some(fp)
    }

    pub fn insert(&self, slot: &SlotGuard, team: &str, scope: &str, fp: [u8; 32]) {
        let key = (slot.index, team.to_string(), scope.to_string());
        self.inner.lock().unwrap().insert(key, fp);
    }
}

type Key = (String, String);
type SlotArray = [bool; SLOTS_PER_SCOPE];

#[derive(Clone, Default)]
pub struct SlotTable {
    inner: Arc<Mutex<HashMap<Key, SlotArray>>>,
}

impl SlotTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_acquire(&self, team: &str, scope: &str, login: &str) -> Option<SlotGuard> {
        let key = (team.to_string(), scope.to_string());
        let mut table = self.inner.lock().unwrap();
        let slots = table.entry(key.clone()).or_insert([false; SLOTS_PER_SCOPE]);
        let preferred = slot_for_user(login);
        let mut order = std::iter::once(preferred)
            .chain((0..SLOTS_PER_SCOPE).filter(move |i| *i != preferred));
        let index = order.find(|&i| !slots[i])?;
        slots[index] = true;
        Some(SlotGuard {
            table: self.inner.clone(),
            key,
            index,
        })
    }
}

pub struct SlotGuard {
    table: Arc<Mutex<HashMap<Key, SlotArray>>>,
    key: Key,
    pub index: usize,
}

impl SlotGuard {
    pub fn workspace(&self, team: &str, scope: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        PathBuf::from(format!("{home}/{team}_{scope}/slot{}", self.index))
    }

    pub fn tmpfs_target(&self, team: &str, scope: &str) -> PathBuf {
        PathBuf::from(format!(
            "/dev/shm/abrasive-targets/{team}_{scope}/slot{}",
            self.index
        ))
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Ok(mut table) = self.table.lock() {
            if let Some(slots) = table.get_mut(&self.key) {
                slots[self.index] = false;
            }
        }
    }
}

/// FNV-1a so the affinity is stable across daemon restarts (the std
/// `DefaultHasher` reseeds per process and would shuffle slots on
/// every reboot).
fn slot_for_user(login: &str) -> usize {
    let mut h: u64 = 14695981039346656037;
    for b in login.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    (h % SLOTS_PER_SCOPE as u64) as usize
}
