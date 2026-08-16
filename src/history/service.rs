use crate::{image_store::MAX_IMAGE_STORAGE_BYTES, storage::StoragePaths};

use super::{
    HistoryItemId, HistoryQuery, ImageData, TextHistoryItem,
    repository::{PersistenceError, PersistenceMutation, PersistenceWorker},
};

const DEFAULT_UNPINNED_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryUpdate {
    Inserted,
    MovedToFront,
    Unchanged,
    Rejected,
}

impl HistoryUpdate {
    pub fn changed(self) -> bool {
        matches!(self, Self::Inserted | Self::MovedToFront)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryChange {
    Applied,
    Rejected,
}

impl HistoryChange {
    pub fn changed(self) -> bool {
        matches!(self, Self::Applied)
    }
}

/// Unified source of truth for text and image clipboard history.
pub struct TextHistory {
    items: Vec<TextHistoryItem>,
    next_id: Option<i64>,
    next_sequence: Option<i64>,
    unpinned_limit: usize,
    image_storage_limit: u64,
    persistence: Option<PersistenceWorker>,
}

impl Default for TextHistory {
    fn default() -> Self {
        Self::from_items(
            Vec::new(),
            DEFAULT_UNPINNED_LIMIT,
            MAX_IMAGE_STORAGE_BYTES,
            None,
        )
    }
}

impl TextHistory {
    pub(crate) fn persistent(paths: StoragePaths) -> Result<Self, PersistenceError> {
        let (persistence, items) = PersistenceWorker::open(paths)?;
        Ok(Self::from_items(
            items,
            DEFAULT_UNPINNED_LIMIT,
            MAX_IMAGE_STORAGE_BYTES,
            Some(persistence),
        ))
    }

    fn from_items(
        mut items: Vec<TextHistoryItem>,
        unpinned_limit: usize,
        image_storage_limit: u64,
        persistence: Option<PersistenceWorker>,
    ) -> Self {
        sort_items(&mut items);
        let next_id = next_value(items.iter().map(|item| item.id().value()));
        let next_sequence = next_value(items.iter().map(TextHistoryItem::last_used_sequence));
        let mut history = Self {
            items,
            next_id,
            next_sequence,
            unpinned_limit,
            image_storage_limit,
            persistence,
        };
        let mut removed_ids = history.enforce_image_storage_limit();
        removed_ids.extend(history.enforce_retention());
        removed_ids.sort_unstable_by_key(|id| id.value());
        removed_ids.dedup();
        if !removed_ids.is_empty() {
            history.persist(PersistenceMutation::Delete { removed_ids });
        }
        history
    }

    pub fn record(&mut self, text: String) -> HistoryUpdate {
        if let Some(index) = self
            .items
            .iter()
            .position(|item| item.as_text().is_some_and(|current| current == text))
        {
            return self.refresh_existing(index);
        }
        let Some((id, sequence)) = self.take_identity() else {
            return HistoryUpdate::Rejected;
        };
        let item = TextHistoryItem::new(HistoryItemId::new(id), text, sequence, sequence, false);
        self.items.push(item.clone());
        sort_items(&mut self.items);
        let removed_ids = self.enforce_retention();
        self.persist(PersistenceMutation::Upsert { item, removed_ids });
        HistoryUpdate::Inserted
    }

    pub fn record_image(&mut self, image: ImageData) -> HistoryUpdate {
        if let Some(index) = self.items.iter().position(|item| {
            item.image()
                .is_some_and(|current| current.content_hash() == image.content_hash())
        }) {
            return self.refresh_existing(index);
        }

        let Some(evictions) = self.plan_image_evictions(image.byte_length()) else {
            eprintln!("lionclip: history image rejected reason=aggregate-storage-limit");
            return HistoryUpdate::Rejected;
        };
        let Some((id, sequence)) = self.take_identity() else {
            return HistoryUpdate::Rejected;
        };

        let mut removed_ids = self.remove_ids(&evictions);
        let item =
            TextHistoryItem::new_image(HistoryItemId::new(id), image, sequence, sequence, false);
        self.items.push(item.clone());
        sort_items(&mut self.items);
        removed_ids.extend(self.enforce_retention());
        self.persist(PersistenceMutation::Upsert { item, removed_ids });
        HistoryUpdate::Inserted
    }

    pub fn items(&self) -> &[TextHistoryItem] {
        &self.items
    }

    pub fn item(&self, id: HistoryItemId) -> Option<&TextHistoryItem> {
        self.items.iter().find(|item| item.id() == id)
    }

    pub fn contains_image_hash(&self, content_hash: &str) -> bool {
        self.items.iter().any(|item| {
            item.image()
                .is_some_and(|image| image.content_hash() == content_hash)
        })
    }

    pub fn search(&self, query: &HistoryQuery) -> Vec<&TextHistoryItem> {
        query.filter(&self.items)
    }

    pub fn pin(&mut self, id: HistoryItemId) -> HistoryChange {
        self.set_pinned(id, true)
    }

    pub fn unpin(&mut self, id: HistoryItemId) -> HistoryChange {
        self.set_pinned(id, false)
    }

    pub fn delete(&mut self, id: HistoryItemId) -> HistoryChange {
        let Some(index) = self.items.iter().position(|item| item.id() == id) else {
            return HistoryChange::Rejected;
        };
        self.items.remove(index);
        self.persist(PersistenceMutation::Delete {
            removed_ids: vec![id],
        });
        HistoryChange::Applied
    }

    pub fn clear_unpinned(&mut self) -> HistoryChange {
        if !self.has_unpinned() {
            return HistoryChange::Rejected;
        }
        self.items.retain(TextHistoryItem::is_pinned);
        self.persist(PersistenceMutation::ClearUnpinned);
        HistoryChange::Applied
    }

    pub fn has_unpinned(&self) -> bool {
        self.items.iter().any(|item| !item.is_pinned())
    }

    pub(crate) fn shutdown_persistence(&mut self) {
        self.persistence.take();
    }

    fn take_identity(&mut self) -> Option<(i64, i64)> {
        let id = take_next(&mut self.next_id)?;
        let sequence = take_next(&mut self.next_sequence)?;
        Some((id, sequence))
    }

    fn refresh_existing(&mut self, index: usize) -> HistoryUpdate {
        if self.items[index].last_used_sequence() == self.newest_sequence() {
            return HistoryUpdate::Unchanged;
        }
        let Some(sequence) = take_next(&mut self.next_sequence) else {
            return HistoryUpdate::Rejected;
        };
        self.items[index].set_last_used_sequence(sequence);
        let item = self.items[index].clone();
        sort_items(&mut self.items);
        self.persist(PersistenceMutation::Upsert {
            item,
            removed_ids: Vec::new(),
        });
        HistoryUpdate::MovedToFront
    }

    fn set_pinned(&mut self, id: HistoryItemId, pinned: bool) -> HistoryChange {
        let Some(index) = self.items.iter().position(|item| item.id() == id) else {
            return HistoryChange::Rejected;
        };
        if self.items[index].is_pinned() == pinned {
            return HistoryChange::Rejected;
        }
        self.items[index].set_pinned(pinned);
        let item = self.items[index].clone();
        sort_items(&mut self.items);
        let mut removed_ids = if pinned {
            Vec::new()
        } else {
            self.enforce_image_storage_limit()
        };
        removed_ids.extend(self.enforce_retention());
        self.persist(PersistenceMutation::Upsert { item, removed_ids });
        HistoryChange::Applied
    }

    fn newest_sequence(&self) -> i64 {
        self.items
            .iter()
            .map(TextHistoryItem::last_used_sequence)
            .max()
            .unwrap_or(i64::MIN)
    }

    fn enforce_retention(&mut self) -> Vec<HistoryItemId> {
        let mut unpinned_seen = 0;
        let mut remove_indices = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.is_pinned() {
                continue;
            }
            unpinned_seen += 1;
            if unpinned_seen > self.unpinned_limit {
                remove_indices.push(index);
            }
        }
        remove_indices
            .into_iter()
            .rev()
            .map(|index| self.items.remove(index).id())
            .collect()
    }

    fn enforce_image_storage_limit(&mut self) -> Vec<HistoryItemId> {
        let mut total = self.image_bytes();
        if total <= self.image_storage_limit {
            return Vec::new();
        }
        let mut ids = Vec::new();
        for item in self.items.iter().rev() {
            if total <= self.image_storage_limit {
                break;
            }
            if item.is_pinned() {
                continue;
            }
            if let Some(image) = item.image() {
                total = total.saturating_sub(image.byte_length());
                ids.push(item.id());
            }
        }
        self.remove_ids(&ids)
    }

    fn plan_image_evictions(&self, incoming_bytes: u64) -> Option<Vec<HistoryItemId>> {
        if incoming_bytes > self.image_storage_limit {
            return None;
        }
        let overflow = self
            .image_bytes()
            .saturating_add(incoming_bytes)
            .saturating_sub(self.image_storage_limit);
        if overflow == 0 {
            return Some(Vec::new());
        }
        let mut freed = 0_u64;
        let mut ids = Vec::new();
        for item in self.items.iter().rev() {
            if item.is_pinned() {
                continue;
            }
            if let Some(image) = item.image() {
                ids.push(item.id());
                freed = freed.saturating_add(image.byte_length());
                if freed >= overflow {
                    return Some(ids);
                }
            }
        }
        None
    }

    fn image_bytes(&self) -> u64 {
        self.items
            .iter()
            .filter_map(TextHistoryItem::image)
            .map(ImageData::byte_length)
            .fold(0_u64, u64::saturating_add)
    }

    fn remove_ids(&mut self, ids: &[HistoryItemId]) -> Vec<HistoryItemId> {
        let mut removed = Vec::new();
        self.items.retain(|item| {
            if ids.contains(&item.id()) {
                removed.push(item.id());
                false
            } else {
                true
            }
        });
        removed
    }

    fn persist(&self, mutation: PersistenceMutation) {
        if let Some(persistence) = &self.persistence {
            persistence.submit(mutation);
        }
    }
}

fn sort_items(items: &mut [TextHistoryItem]) {
    items.sort_by_key(|item| {
        (
            std::cmp::Reverse(item.is_pinned()),
            std::cmp::Reverse(item.last_used_sequence()),
            std::cmp::Reverse(item.id().value()),
        )
    });
}

fn next_value(values: impl Iterator<Item = i64>) -> Option<i64> {
    match values.max() {
        Some(value) => value.checked_add(1),
        None => Some(0),
    }
}

fn take_next(next: &mut Option<i64>) -> Option<i64> {
    let current = next.take()?;
    *next = current.checked_add(1);
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::ImageMime;

    fn image(key: char, bytes: u64) -> ImageData {
        ImageData::new(
            key.to_string().repeat(64),
            ImageMime::Png,
            bytes,
            1920,
            1080,
        )
    }

    #[test]
    fn text_dedup_and_order_are_unchanged() {
        let mut history = TextHistory::default();
        history.record("A".into());
        let id = history.items()[0].id();
        history.record("B".into());
        assert_eq!(history.record("A".into()), HistoryUpdate::MovedToFront);
        assert_eq!(history.items()[0].as_text(), Some("A"));
        assert_eq!(history.items()[0].id(), id);
    }

    #[test]
    fn mixed_payloads_share_one_order() {
        let mut history = TextHistory::default();
        history.record("A".into());
        history.record_image(image('a', 100));
        history.record("B".into());
        assert_eq!(history.items()[0].as_text(), Some("B"));
        assert!(history.items()[1].image().is_some());
        assert_eq!(history.items()[2].as_text(), Some("A"));
    }

    #[test]
    fn image_dedup_reuses_identity() {
        let mut history = TextHistory::default();
        history.record_image(image('a', 100));
        let id = history.items()[0].id();
        history.record("later".into());
        assert_eq!(
            history.record_image(image('a', 100)),
            HistoryUpdate::MovedToFront
        );
        assert_eq!(history.items()[0].id(), id);
        assert_eq!(history.items().len(), 2);
    }

    #[test]
    fn storage_cap_evicts_oldest_unpinned_image() {
        let mut history = TextHistory::from_items(Vec::new(), 500, 150, None);
        history.record_image(image('a', 100));
        history.record_image(image('b', 100));
        assert_eq!(history.items().len(), 1);
        assert!(history.contains_image_hash(&"b".repeat(64)));
    }

    #[test]
    fn storage_cap_never_evicts_pinned_image_for_new_capture() {
        let mut history = TextHistory::from_items(Vec::new(), 500, 150, None);
        history.record_image(image('a', 100));
        let id = history.items()[0].id();
        history.pin(id);
        assert_eq!(
            history.record_image(image('b', 100)),
            HistoryUpdate::Rejected
        );
        assert_eq!(history.items().len(), 1);
        assert_eq!(history.items()[0].id(), id);
    }
}
