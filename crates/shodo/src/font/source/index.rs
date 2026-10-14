//! One shared catalog snapshot, with incremental explicitly registered faces.

use super::{
    FontCollection, FontData, FontError, LookupWork, admit_name_record, decode_name,
    has_local_name, local_name_records,
};
use crate::font::LayerState;
use crate::limits::{LimitKind, Limits};
use fontique::{FontInfo, SourceId, SourceKind};
use skrifa::{FontRef, raw::TableProvider};
use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
    sync::Arc,
};

crate::logical::logical_sizes! {
    FONT_INFO_BYTES: FontInfo = 96;
}

// Include hash bucket slack and conservative per-entry metadata. Work is
// charged even for duplicate names; storage is charged only on admission.
const ENTRY_BYTES: u64 = 256;
const IDENTITY_BYTES: u64 = 64;
// Vec allocation/growth slack, including the old allocation during growth.
const CANDIDATE_BYTES: u64 = 64;

#[derive(Default)]
pub(in crate::font) struct LocalNames {
    registered: HashMap<String, usize>,
    native: HashMap<String, Vec<Arc<FontInfo>>>,
    registered_faces: usize,
    registered_error: Option<FontError>,
    native_result: Option<Result<(), FontError>>,
    seen: HashSet<(SourceId, u32)>,
    budget: IndexBudget,
}

#[derive(Clone, Default)]
struct IndexBudget {
    bytes: u64,
    temporary: u64,
    work: LookupWork,
}

pub(in crate::font) struct PreparedRegistration {
    names: HashMap<String, usize>,
    seen: HashSet<(SourceId, u32)>,
    budget: IndexBudget,
    end: usize,
}

impl IndexBudget {
    fn check_bytes(&self, extra: u64, limits: &Limits) -> Result<(), FontError> {
        Limits::check(
            limits.max_local_font_index_bytes,
            LimitKind::LocalFontIndexBytes,
            self.bytes
                .saturating_add(self.temporary)
                .saturating_add(extra),
        )?;
        Ok(())
    }

    fn retain(&mut self, extra: u64, limits: &Limits) -> Result<(), FontError> {
        self.check_bytes(extra, limits)?;
        self.bytes = self.bytes.saturating_add(extra);
        Ok(())
    }
}

#[derive(Clone)]
enum Target {
    Registered(usize),
    Native(Arc<FontInfo>),
}

impl LocalNames {
    /// Once local lookup starts, publish registered faces and their names
    /// together. A failed addition must not obscure existing native names or
    /// leave unknown registered names ahead of them in precedence order.
    pub(in crate::font) fn prepare_registration(
        &self,
        state: &LayerState,
        blob: &peniko::Blob<u8>,
        first_index: u32,
        faces: u32,
        limits: &Limits,
    ) -> Result<Option<PreparedRegistration>, FontError> {
        if self.registered_faces == 0 && self.registered_error.is_none() {
            return Ok(None);
        }
        if let Some(error) = self.registered_error {
            return Err(error);
        }
        let mut staged = Self {
            budget: self.budget.clone(),
            ..Self::default()
        };
        for slot in self.registered_faces..state.faces.len() {
            if state.descriptors[slot].is_none() {
                staged.budget.work.charge(1, limits)?;
                staged.add_names(&state.faces[slot], Target::Registered(slot), limits)?;
                if let Some(info) = &state.face_infos[slot]
                    && !self.seen.contains(&(info.source().id(), info.index()))
                {
                    staged.mark_seen(info, limits)?;
                }
            }
        }
        let first = state.faces.len();
        for index in 0..faces {
            staged.budget.work.charge(1, limits)?;
            staged.add_names(
                &FontData::new(blob.clone(), first_index + index),
                Target::Registered(first + index as usize),
                limits,
            )?;
            // Source identities become available only after native registration.
            staged.budget.retain(IDENTITY_BYTES, limits)?;
        }
        Ok(Some(PreparedRegistration {
            names: staged.registered,
            seen: staged.seen,
            budget: staged.budget,
            end: first + faces as usize,
        }))
    }

    pub(in crate::font) fn commit_registration(
        &mut self,
        prepared: PreparedRegistration,
        infos: &[Option<FontInfo>],
    ) {
        for (name, slot) in prepared.names {
            self.registered.entry(name).or_insert(slot);
        }
        self.seen.extend(prepared.seen);
        for info in infos.iter().flatten() {
            self.seen.insert((info.source().id(), info.index()));
        }
        self.budget = prepared.budget;
        self.registered_faces = prepared.end;
    }

    pub(super) fn lookup(
        &mut self,
        root: &FontCollection,
        state: &mut LayerState,
        name: &str,
        verification: &mut LookupWork,
        caller_limits: &Limits,
    ) -> Result<Option<(FontData, Option<SourceId>)>, FontError> {
        // Appended faces cannot displace an already indexed registered face.
        if let Some(&slot) = self.registered.get(name) {
            return Ok(state.faces.get(slot).cloned().map(|data| (data, None)));
        }
        let limits = &root.layer.limits;
        if let Some(error) = self.registered_error {
            return Err(error);
        }
        if let Err(error) = self.index_registered(state, limits) {
            self.registered_error = Some(error);
            if let Some(&slot) = self.registered.get(name) {
                return Ok(state.faces.get(slot).cloned().map(|data| (data, None)));
            }
            return Err(error);
        }
        if let Some(&slot) = self.registered.get(name) {
            return Ok(state.faces.get(slot).cloned().map(|data| (data, None)));
        }
        if self.native_result.is_none() {
            root.ensure_system(state);
            let result = self.index_native(state, limits);
            self.budget.temporary = 0;
            self.native_result = Some(result);
        }
        let Some(infos) = self.native.get(name) else {
            return self.native_result.unwrap_or(Ok(())).map(|()| None);
        };
        for info in infos {
            verification.charge(1, caller_limits)?;
            // Load only exact-name candidates without scanning or pruning the
            // platform SourceCache. Retained root/document source identities
            // supply blob sharing; the name index owns no mapped font data.
            let blob = state
                .retained_sources
                .get(&info.source().id())
                .cloned()
                .or_else(|| info.load(None));
            let Some(blob) = blob else {
                continue;
            };
            let data = FontData::new(blob, info.index());
            // Memory catalog sources are immutable. Path sources may have
            // changed; continue to the next exact-name candidate in that case.
            if matches!(info.source().kind(), SourceKind::Path(_))
                && !has_local_name(&data, name, verification, caller_limits)?
            {
                continue;
            }
            return Ok(Some((data, Some(info.source().id()))));
        }
        // A failed partial catalog cannot establish that no later face matches.
        self.native_result.unwrap_or(Ok(())).map(|()| None)
    }

    fn index_registered(&mut self, state: &LayerState, limits: &Limits) -> Result<(), FontError> {
        while self.registered_faces < state.faces.len() {
            let slot = self.registered_faces;
            if state.descriptors[slot].is_none() {
                self.budget.work.charge(1, limits)?;
                self.add_names(&state.faces[slot], Target::Registered(slot), limits)?;
                if let Some(info) = state.face_infos[slot].as_ref() {
                    self.mark_seen(info, limits)?;
                }
            }
            self.registered_faces += 1;
        }
        Ok(())
    }

    fn mark_seen(&mut self, info: &FontInfo, limits: &Limits) -> Result<bool, FontError> {
        let key = (info.source().id(), info.index());
        if self.seen.contains(&key) {
            return Ok(false);
        }
        self.budget.retain(IDENTITY_BYTES, limits)?;
        self.seen.insert(key);
        Ok(true)
    }

    fn index_native(&mut self, state: &mut LayerState, limits: &Limits) -> Result<(), FontError> {
        let mut families = Vec::new();
        for name in state.native.family_names() {
            self.budget.work.charge(1 + name.len() as u64, limits)?;
            // Account the full snapshot, including Vec growth slack, before
            // copying any name. Release construction scratch on all outcomes.
            let cost = name.len() as u64 + 3 * crate::logical::VEC_HEADER;
            self.budget.check_bytes(cost, limits)?;
            self.budget.temporary = self.budget.temporary.saturating_add(cost);
            families.push(name.to_owned());
        }
        for name in families {
            self.budget.work.charge(1, limits)?;
            let Some(family) = state.native.family_by_name(&name) else {
                continue;
            };
            for info in family.fonts() {
                self.budget.work.charge(1, limits)?;
                if !self.mark_seen(info, limits)? {
                    continue;
                }
                self.budget.work.charge(1, limits)?;
                #[cfg(test)]
                super::LOCAL_CATALOG_LOADS.with(|calls| calls.set(calls.get() + 1));
                // Do not retain every installed blob while indexing names.
                let Some(blob) = state
                    .retained_sources
                    .get(&info.source().id())
                    .cloned()
                    .or_else(|| info.load(None))
                else {
                    continue;
                };
                #[cfg(test)]
                if matches!(info.source().kind(), SourceKind::Path(_)) {
                    super::LOCAL_INDEX_BLOBS
                        .with(|blobs| blobs.borrow_mut().push(blob.downgrade()));
                }
                let metadata = FONT_INFO_BYTES
                    + 2 * crate::logical::WORD
                    + if info.axes().len() > 1 {
                        (2 * info.axes().len() * size_of::<fontique::AxisInfo>()) as u64
                    } else {
                        0
                    };
                self.budget.retain(metadata, limits)?;
                let target = Target::Native(Arc::new(info.clone()));
                self.add_names(&FontData::new(blob, info.index()), target, limits)?;
            }
        }
        Ok(())
    }

    fn add_names(
        &mut self,
        data: &FontData,
        target: Target,
        limits: &Limits,
    ) -> Result<(), FontError> {
        #[cfg(test)]
        super::LOCAL_NAME_FACE_READS.with(|calls| calls.set(calls.get() + 1));
        let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
            return Ok(());
        };
        let Ok(table) = font.name() else {
            return Ok(());
        };
        for record in local_name_records(&table, &mut self.budget.work, limits)? {
            let scratch = admit_name_record(record, &mut self.budget.work, limits)?;
            self.budget.check_bytes(scratch, limits)?;
            let name = decode_name(&table, record);
            let exists = match &target {
                Target::Registered(_) => self.registered.contains_key(&name),
                Target::Native(info) => self
                    .native
                    .get(&name)
                    .and_then(|infos| infos.last())
                    .is_some_and(|last| Arc::ptr_eq(last, info)),
            };
            if exists {
                continue;
            }
            match &target {
                Target::Registered(slot) => {
                    self.budget
                        .retain(name.capacity() as u64 + ENTRY_BYTES, limits)?;
                    self.registered.insert(name, *slot);
                }
                Target::Native(info) => {
                    let key_bytes = if self.native.contains_key(&name) {
                        0
                    } else {
                        name.capacity() as u64 + ENTRY_BYTES
                    };
                    self.budget.retain(key_bytes + CANDIDATE_BYTES, limits)?;
                    self.native.entry(name).or_default().push(info.clone());
                }
            }
        }
        Ok(())
    }
}
