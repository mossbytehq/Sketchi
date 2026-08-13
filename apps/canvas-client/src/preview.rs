//! Bounded background work for hovered image previews.

use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

use crate::images::{ImageImportError, embedded_image_with_rgba};
use canvas_core::MAX_IMAGE_BYTES;

/// Result of decoding one hovered image.
pub(crate) struct DropPreviewDecode {
    /// Source path.
    pub(crate) path: PathBuf,
    /// Decoded document payload and RGBA pixels.
    pub(crate) result: Result<(canvas_core::EmbeddedImage, Vec<u8>), DropPreviewDecodeError>,
}

/// Errors raised while preparing a hovered preview.
pub(crate) enum DropPreviewDecodeError {
    /// The source file could not be read.
    Read(String),
    /// The source bytes were not a supported bounded image.
    Decode(ImageImportError),
}

/// Cooperative cancellation token for one queued preview job.
pub(crate) struct PreviewCancellation(Arc<AtomicBool>);

impl PreviewCancellation {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for PreviewCancellation {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct PreviewJob {
    path: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: mpsc::Sender<DropPreviewDecode>,
}

struct PreviewWorkerInner {
    pending: Mutex<Option<PreviewJob>>,
    wake: Condvar,
    stopping: AtomicBool,
}

/// One bounded worker that always keeps only the newest pending hover.
pub(crate) struct PreviewWorker {
    inner: Arc<PreviewWorkerInner>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PreviewWorker {
    pub(crate) fn new() -> Self {
        let inner = Arc::new(PreviewWorkerInner {
            pending: Mutex::new(None),
            wake: Condvar::new(),
            stopping: AtomicBool::new(false),
        });
        let worker_inner = Arc::clone(&inner);
        let thread = std::thread::Builder::new()
            .name("sketchi-image-preview".to_owned())
            .spawn(move || run_worker(&worker_inner))
            .ok();
        Self { inner, thread }
    }

    pub(crate) fn queue(
        &self,
        path: PathBuf,
    ) -> (Receiver<DropPreviewDecode>, PreviewCancellation) {
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let job = PreviewJob {
            path,
            cancel: Arc::clone(&cancel),
            sender,
        };
        let mut pending = match self.inner.pending.lock() {
            Ok(pending) => pending,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(previous) = pending.replace(job) {
            previous.cancel.store(true, Ordering::Release);
        }
        self.inner.wake.notify_one();
        (receiver, PreviewCancellation(cancel))
    }
}

impl Drop for PreviewWorker {
    fn drop(&mut self) {
        self.inner.stopping.store(true, Ordering::Release);
        if let Ok(mut pending) = self.inner.pending.lock()
            && let Some(job) = pending.take()
        {
            job.cancel.store(true, Ordering::Release);
        }
        self.inner.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_worker(inner: &PreviewWorkerInner) {
    loop {
        let job = {
            let mut pending = match inner.pending.lock() {
                Ok(pending) => pending,
                Err(poisoned) => poisoned.into_inner(),
            };
            while pending.is_none() && !inner.stopping.load(Ordering::Acquire) {
                pending = match inner.wake.wait(pending) {
                    Ok(pending) => pending,
                    Err(poisoned) => poisoned.into_inner(),
                };
            }
            if inner.stopping.load(Ordering::Acquire) {
                return;
            }
            match pending.take() {
                Some(job) => job,
                None => continue,
            }
        };

        if job.cancel.load(Ordering::Acquire) || inner.stopping.load(Ordering::Acquire) {
            continue;
        }
        tracing::info!(path = %job.path.display(), "preparing hovered image preview");
        let result = match read_preview_bytes(&job.path, &job.cancel, &inner.stopping) {
            Ok(Some(bytes)) => {
                if job.cancel.load(Ordering::Acquire) {
                    continue;
                }
                tracing::info!(
                    path = %job.path.display(),
                    bytes = bytes.len(),
                    "hovered image preview bytes read"
                );
                embedded_image_with_rgba(bytes).map_err(DropPreviewDecodeError::Decode)
            }
            Ok(None) => continue,
            Err(error) => Err(error),
        };
        if !job.cancel.load(Ordering::Acquire) && !inner.stopping.load(Ordering::Acquire) {
            let _ = job.sender.send(DropPreviewDecode {
                path: job.path,
                result,
            });
        }
    }
}

fn read_preview_bytes(
    path: &PathBuf,
    cancelled: &AtomicBool,
    stopping: &AtomicBool,
) -> Result<Option<Vec<u8>>, DropPreviewDecodeError> {
    if cancelled.load(Ordering::Acquire) || stopping.load(Ordering::Acquire) {
        return Ok(None);
    }
    let mut file =
        File::open(path).map_err(|error| DropPreviewDecodeError::Read(error.to_string()))?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) || stopping.load(Ordering::Acquire) {
            return Ok(None);
        }
        let count = file
            .read(&mut chunk)
            .map_err(|error| DropPreviewDecodeError::Read(error.to_string()))?;
        if count == 0 {
            return Ok(Some(bytes));
        }
        if bytes.len().saturating_add(count) > MAX_IMAGE_BYTES {
            return Err(DropPreviewDecodeError::Decode(ImageImportError::TooLarge));
        }
        let Some(data) = chunk.get(..count) else {
            return Err(DropPreviewDecodeError::Read(
                "preview read exceeded its buffer".to_owned(),
            ));
        };
        bytes.extend_from_slice(data);
    }
}

#[cfg(test)]
mod tests {
    use super::{DropPreviewDecodeError, read_preview_bytes};
    use canvas_core::MAX_IMAGE_BYTES;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn preview_reads_are_bounded_before_decode() {
        let path =
            std::env::temp_dir().join(format!("sketchi-preview-test-{}", std::process::id()));
        let write_result = fs::write(&path, vec![0_u8; MAX_IMAGE_BYTES + 1]);
        assert!(write_result.is_ok());
        let cancelled = AtomicBool::new(false);
        let stopping = AtomicBool::new(false);

        let result = read_preview_bytes(&path, &cancelled, &stopping);

        assert!(matches!(
            result,
            Err(DropPreviewDecodeError::Decode(
                crate::images::ImageImportError::TooLarge
            ))
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn cancelled_preview_does_not_read_the_file() {
        let path = PathBuf::from("/definitely/not/a/preview");
        let cancelled = AtomicBool::new(true);
        let stopping = AtomicBool::new(false);

        let result = read_preview_bytes(&path, &cancelled, &stopping);

        assert!(matches!(result, Ok(None)));
        assert!(cancelled.load(Ordering::Acquire));
    }
}
