use std::{
    cell::Cell,
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use gtk::{
    gdk_pixbuf::{InterpType, PixbufLoader, prelude::*},
    glib,
};

use crate::{
    history::{HistoryItem, HistoryItemId, ImageData, ImageMime},
    storage::StoragePaths,
};

pub const MAX_IMAGE_ENCODED_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;
pub const MAX_IMAGE_STORAGE_BYTES: u64 = 512 * 1024 * 1024;

const THUMBNAIL_MAX_WIDTH: u32 = 240;
const THUMBNAIL_MAX_HEIGHT: u32 = 135;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageStoreError {
    TooLarge,
    InvalidImage,
    InvalidDimensions,
    Checksum,
    Directory,
    Write,
    Thumbnail,
}

impl ImageStoreError {
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::TooLarge => "image-too-large",
            Self::InvalidImage => "image-invalid",
            Self::InvalidDimensions => "image-dimensions-invalid",
            Self::Checksum => "image-checksum",
            Self::Directory => "image-directory",
            Self::Write => "image-write",
            Self::Thumbnail => "image-thumbnail",
        }
    }
}

#[derive(Clone, Debug)]
pub struct StoredImage {
    pub image: ImageData,
    /// True only when this capture created the content-addressed original.
    /// It lets stale clipboard work remove its own orphan without touching a
    /// blob that was already referenced by history.
    pub original_created: bool,
}

pub fn process_and_store(
    paths: &StoragePaths,
    mime_type: ImageMime,
    bytes: Vec<u8>,
) -> Result<StoredImage, ImageStoreError> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_ENCODED_BYTES {
        return Err(ImageStoreError::TooLarge);
    }

    let (width, height, thumbnail) = decode_thumbnail(mime_type, &bytes)?;
    let content_hash = glib::compute_checksum_for_data(glib::ChecksumType::Sha256, &bytes)
        .ok_or(ImageStoreError::Checksum)?
        .to_string();
    let byte_length = u64::try_from(bytes.len()).map_err(|_| ImageStoreError::TooLarge)?;
    let image = ImageData::new(content_hash, mime_type, byte_length, width, height);

    ensure_private_directory(paths.blobs())?;
    ensure_private_directory(paths.thumbnails())?;

    let original_path = blob_path(paths, &image).ok_or(ImageStoreError::Checksum)?;
    let thumbnail_path = thumbnail_path(paths, &image).ok_or(ImageStoreError::Checksum)?;
    let original_created = write_atomic_if_missing(&original_path, &bytes)?;
    if let Err(error) = write_atomic_if_missing(&thumbnail_path, &thumbnail) {
        if original_created {
            let _ = fs::remove_file(&original_path);
        }
        return Err(error);
    }

    Ok(StoredImage {
        image,
        original_created,
    })
}

pub fn blob_path(paths: &StoragePaths, image: &ImageData) -> Option<PathBuf> {
    valid_hash(image.content_hash()).then(|| {
        paths.blobs().join(format!(
            "{}.{}",
            image.content_hash(),
            image.mime_type().extension()
        ))
    })
}

pub fn thumbnail_path(paths: &StoragePaths, image: &ImageData) -> Option<PathBuf> {
    valid_hash(image.content_hash())
        .then(|| paths.thumbnails().join(format!("{}.png", image.content_hash())))
}

pub fn delete_asset(paths: &StoragePaths, image: &ImageData) {
    if let Some(path) = blob_path(paths, image) {
        remove_file_best_effort(&path, "image-blob-delete");
    }
    if let Some(path) = thumbnail_path(paths, image) {
        remove_file_best_effort(&path, "image-thumbnail-delete");
    }
}

/// Removes LionClip-owned files that no database item references and reports
/// image rows whose original blob disappeared. It never follows paths supplied
/// by the database: only validated content hashes are mapped into owned dirs.
pub fn reconcile(
    paths: &StoragePaths,
    items: &[HistoryItem],
) -> Result<Vec<HistoryItemId>, ImageStoreError> {
    ensure_private_directory(paths.blobs())?;
    ensure_private_directory(paths.thumbnails())?;

    let mut expected_blobs = HashSet::new();
    let mut expected_thumbnails = HashSet::new();
    let mut missing = Vec::new();

    for item in items {
        let Some(image) = item.image() else {
            continue;
        };
        let Some(blob) = blob_path(paths, image) else {
            missing.push(item.id());
            continue;
        };
        let Some(thumbnail) = thumbnail_path(paths, image) else {
            missing.push(item.id());
            continue;
        };

        expected_blobs.insert(blob.clone());
        expected_thumbnails.insert(thumbnail.clone());
        if !blob.is_file() {
            missing.push(item.id());
            let _ = fs::remove_file(thumbnail);
        }
    }

    remove_orphans(paths.blobs(), &expected_blobs)?;
    remove_orphans(paths.thumbnails(), &expected_thumbnails)?;
    Ok(missing)
}

fn decode_thumbnail(
    mime_type: ImageMime,
    bytes: &[u8],
) -> Result<(u32, u32, Vec<u8>), ImageStoreError> {
    let loader = PixbufLoader::with_mime_type(mime_type.as_str())
        .map_err(|_| ImageStoreError::InvalidImage)?;
    let dimensions = Rc::new(Cell::new(None::<(u32, u32)>));
    let invalid_dimensions = Rc::new(Cell::new(false));

    loader.connect_size_prepared({
        let dimensions = dimensions.clone();
        let invalid_dimensions = invalid_dimensions.clone();

        move |loader, width, height| {
            let Ok(width) = u32::try_from(width) else {
                invalid_dimensions.set(true);
                loader.set_size(1, 1);
                return;
            };
            let Ok(height) = u32::try_from(height) else {
                invalid_dimensions.set(true);
                loader.set_size(1, 1);
                return;
            };
            dimensions.set(Some((width, height)));

            if !dimensions_allowed(width, height) {
                invalid_dimensions.set(true);
                loader.set_size(1, 1);
                return;
            }

            let (thumb_width, thumb_height) = thumbnail_dimensions(width, height);
            let thumb_width = i32::try_from(thumb_width).unwrap_or(1);
            let thumb_height = i32::try_from(thumb_height).unwrap_or(1);
            loader.set_size(thumb_width, thumb_height);
        }
    });

    loader
        .write(bytes)
        .map_err(|_| ImageStoreError::InvalidImage)?;
    loader.close().map_err(|_| ImageStoreError::InvalidImage)?;

    let (width, height) = dimensions.get().ok_or(ImageStoreError::InvalidImage)?;
    if invalid_dimensions.get() || !dimensions_allowed(width, height) {
        return Err(ImageStoreError::InvalidDimensions);
    }

    let pixbuf = loader.pixbuf().ok_or(ImageStoreError::InvalidImage)?;
    let thumbnail = pixbuf
        .save_to_bufferv("png", &[])
        .map_err(|_| ImageStoreError::Thumbnail)?;
    Ok((width, height, thumbnail))
}

fn dimensions_allowed(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_IMAGE_DIMENSION
        && height <= MAX_IMAGE_DIMENSION
        && u64::from(width).saturating_mul(u64::from(height)) <= MAX_IMAGE_PIXELS
}

fn thumbnail_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width <= THUMBNAIL_MAX_WIDTH && height <= THUMBNAIL_MAX_HEIGHT {
        return (width.max(1), height.max(1));
    }

    let width_limited = u64::from(width) * u64::from(THUMBNAIL_MAX_HEIGHT)
        >= u64::from(height) * u64::from(THUMBNAIL_MAX_WIDTH);
    if width_limited {
        let scaled_height = (u64::from(height) * u64::from(THUMBNAIL_MAX_WIDTH)
            / u64::from(width))
        .max(1);
        (THUMBNAIL_MAX_WIDTH, scaled_height as u32)
    } else {
        let scaled_width = (u64::from(width) * u64::from(THUMBNAIL_MAX_HEIGHT)
            / u64::from(height))
        .max(1);
        (scaled_width as u32, THUMBNAIL_MAX_HEIGHT)
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), ImageStoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(path)
        .map_err(|_| ImageStoreError::Directory)
}

fn write_atomic_if_missing(path: &Path, bytes: &[u8]) -> Result<bool, ImageStoreError> {
    if path.is_file() {
        return Ok(false);
    }
    let parent = path.parent().ok_or(ImageStoreError::Directory)?;
    ensure_private_directory(parent)?;

    let suffix = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".lionclip-{}-{suffix}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| ImageStoreError::Write)?;
        file.write_all(bytes).map_err(|_| ImageStoreError::Write)?;
        file.sync_all().map_err(|_| ImageStoreError::Write)?;
        drop(file);
        fs::rename(&temp, path).map_err(|_| ImageStoreError::Write)?;
        Ok(true)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn remove_orphans(directory: &Path, expected: &HashSet<PathBuf>) -> Result<(), ImageStoreError> {
    let entries = fs::read_dir(directory).map_err(|_| ImageStoreError::Directory)?;
    for entry in entries {
        let entry = entry.map_err(|_| ImageStoreError::Directory)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|_| ImageStoreError::Directory)?;
        if (file_type.is_file() || file_type.is_symlink()) && !expected.contains(&path) {
            remove_file_best_effort(&path, "image-orphan-delete");
        }
    }
    Ok(())
}

fn remove_file_best_effort(path: &Path, stage: &str) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("lionclip: image storage cleanup failed stage={stage}");
    }
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_size_preserves_aspect_ratio_and_bounds() {
        assert_eq!(thumbnail_dimensions(1920, 1080), (240, 135));
        assert_eq!(thumbnail_dimensions(1080, 1920), (76, 135));
        assert_eq!(thumbnail_dimensions(100, 50), (100, 50));
    }

    #[test]
    fn image_dimension_limits_are_explicit() {
        assert!(dimensions_allowed(1920, 1080));
        assert!(!dimensions_allowed(0, 1080));
        assert!(!dimensions_allowed(MAX_IMAGE_DIMENSION + 1, 1));
        assert!(!dimensions_allowed(10_000, 10_000));
    }

    #[test]
    fn only_lowercase_sha256_keys_are_accepted_for_paths() {
        assert!(valid_hash(&"a".repeat(64)));
        assert!(valid_hash(&"0".repeat(64)));
        assert!(!valid_hash("../escape"));
        assert!(!valid_hash(&"A".repeat(64)));
        assert!(!valid_hash(&"a".repeat(63)));
    }
}
