use crate::fs::{asyncify, sys};

use futures_core::ready;
use futures_core::stream::Stream;
use std::ffi::OsString;
use std::fs::{FileType, Metadata};
use std::future::Future;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::DirEntryExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

/// Returns a stream over the entries within a directory.
///
/// This is an async version of [`std::fs::read_dir`](std::fs::read_dir)
pub async fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
    let path = path.as_ref().to_owned();
    let std = asyncify(|| std::fs::read_dir(path)).await?;

    Ok(ReadDir(State::Idle(Some(std))))
}

/// Stream of the entries in a directory.
///
/// This stream is returned from the [`read_dir`] function of this module and
/// will yield instances of [`DirEntry`]. Through a [`DirEntry`]
/// information like the entry's path and possibly other metadata can be
/// learned.
///
/// # Errors
///
/// This [`Stream`] will return an [`Err`] if there's some sort of intermittent
/// IO error during iteration.
///
/// [`read_dir`]: read_dir
/// [`DirEntry`]: DirEntry
/// [`Stream`]: Stream
/// [`Err`]: std::result::Result::Err
#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
pub struct ReadDir(State);

#[derive(Debug)]
enum State {
    Idle(Option<std::fs::ReadDir>),
    Pending(sys::Blocking<(Option<io::Result<std::fs::DirEntry>>, std::fs::ReadDir)>),
}

impl Stream for ReadDir {
    type Item = io::Result<DirEntry>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.0 {
                State::Idle(ref mut std) => {
                    let mut std = std.take().unwrap();

                    self.0 = State::Pending(sys::run(move || {
                        let ret = std.next();
                        (ret, std)
                    }));
                }
                State::Pending(ref mut rx) => {
                    let (ret, std) = ready!(Pin::new(rx).poll(cx));
                    self.0 = State::Idle(Some(std));

                    let ret = ret.map(|res| res.map(|std| DirEntry(Arc::new(std))));

                    return Poll::Ready(ret);
                }
            }
        }
    }
}

/// Entries returned by the [`ReadDir`] stream.
///
/// [`ReadDir`]: struct.ReadDir.html
///
/// This is a specialized version of [`std::fs::DirEntry`] for usage from the
/// Tokio runtime.
///
/// An instance of `DirEntry` represents an entry inside of a directory on the
/// filesystem. Each entry can be inspected via methods to learn about the full
/// path or possibly other metadata through per-platform extension traits.
#[derive(Debug)]
pub struct DirEntry(Arc<std::fs::DirEntry>);

impl DirEntry {
    /// Returns the full path to the file that this entry represents.
    ///
    /// The full path is created by joining the original path to `read_dir`
    /// with the filename of this entry.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tokio::fs;
    /// use tokio::prelude::*;
    ///
    /// # async fn dox() -> std::io::Result<()> {
    /// let mut entries = fs::read_dir(".").await?;
    ///
    /// while let Some(res) = entries.next().await {
    ///     let entry = res?;
    ///     println!("{:?}", entry.path());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// This prints output like:
    ///
    /// ```text
    /// "./whatever.txt"
    /// "./foo.html"
    /// "./hello_world.rs"
    /// ```
    ///
    /// The exact text, of course, depends on what files you have in `.`.
    pub fn path(&self) -> PathBuf {
        self.0.path()
    }

    /// Returns the bare file name of this directory entry without any other
    /// leading path component.
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::fs;
    /// use tokio::prelude::*;
    ///
    /// # async fn dox() -> std::io::Result<()> {
    /// let mut entries = fs::read_dir(".").await?;
    ///
    /// while let Some(res) = entries.next().await {
    ///     let entry = res?;
    ///     println!("{:?}", entry.file_name());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn file_name(&self) -> OsString {
        self.0.file_name()
    }

    /// Return the metadata for the file that this entry points at.
    ///
    /// This function will not traverse symlinks if this entry points at a
    /// symlink.
    ///
    /// # Platform-specific behavior
    ///
    /// On Windows this function is cheap to call (no extra system calls
    /// needed), but on Unix platforms this function is the equivalent of
    /// calling `symlink_metadata` on the path.
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::fs;
    /// use tokio::prelude::*;
    ///
    /// # async fn dox() -> std::io::Result<()> {
    /// let mut entries = fs::read_dir(".").await?;
    ///
    /// while let Some(res) = entries.next().await {
    ///     let entry = res?;
    ///
    ///     if let Ok(metadata) = entry.metadata().await {
    ///         // Now let's show our entry's permissions!
    ///         println!("{:?}: {:?}", entry.path(), metadata.permissions());
    ///     } else {
    ///         println!("Couldn't get file type for {:?}", entry.path());
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn metadata(&self) -> io::Result<Metadata> {
        let std = self.0.clone();
        asyncify(move || std.metadata()).await
    }

    /// Return the file type for the file that this entry points at.
    ///
    /// This function will not traverse symlinks if this entry points at a
    /// symlink.
    ///
    /// # Platform-specific behavior
    ///
    /// On Windows and most Unix platforms this function is free (no extra
    /// system calls needed), but some Unix platforms may require the equivalent
    /// call to `symlink_metadata` to learn about the target file type.
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::fs;
    /// use tokio::prelude::*;
    ///
    /// # async fn dox() -> std::io::Result<()> {
    /// let mut entries = fs::read_dir(".").await?;
    ///
    /// while let Some(res) = entries.next().await {
    ///     let entry = res?;
    ///
    ///     if let Ok(file_type) = entry.file_type().await {
    ///         // Now let's show our entry's file type!
    ///         println!("{:?}: {:?}", entry.path(), file_type);
    ///     } else {
    ///         println!("Couldn't get file type for {:?}", entry.path());
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn file_type(&self) -> io::Result<FileType> {
        let std = self.0.clone();
        asyncify(move || std.file_type()).await
    }
}

#[cfg(unix)]
impl DirEntryExt for DirEntry {
    fn ino(&self) -> u64 {
        self.0.ino()
    }
}
