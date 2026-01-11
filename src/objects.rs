use anyhow::Context;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use sha1::Digest;
use sha1::Sha1;
use std::ffi::CStr;
use std::fmt;
use std::fs;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::Path;

/// Enum representing different types of git objects.
///
/// Git stores objects in `.git/objects/` with different types:
/// - Blob: file content
/// - Tree: directory structure
/// - Commit: commit metadata
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Blob,
    Tree,
    Commit,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::Blob => write!(f, "blob"),
            Kind::Tree => write!(f, "tree"),
            Kind::Commit => write!(f, "commit"),
        }
    }
}

/// A git object with a generic reader type.
///
/// This struct abstracts over git objects stored in `.git/objects/`.
/// The reader `R` can be:
/// - A file reader (for creating blobs from files)
/// - A decompressed, size-limited reader (for reading objects from .git/objects/)
///
/// The object format after decompression is: `<kind> <size>\0<content>`
pub(crate) struct Object<R> {
    pub(crate) kind: Kind,
    pub(crate) expected_size: u64,
    pub(crate) reader: R,
}

impl Object<()> {
    /// Create a blob object from a file on disk.
    ///
    /// This reads the file's metadata to get its size, then opens the file for reading.
    /// The returned Object can be used to compute the hash or write to .git/objects/.
    ///
    /// TODO: technically there's a race here if the file changes between stat and write
    pub(crate) fn blob_from_file(file: impl AsRef<Path>) -> anyhow::Result<Object<impl Read>> {
        let file = file.as_ref();
        // Get file metadata to determine the blob size
        let stat = std::fs::metadata(file).with_context(|| format!("stat {}", file.display()))?;
        // TODO: technically there's a race here if the file changes between stat and write
        // Open the file for reading
        let file = std::fs::File::open(file).with_context(|| format!("open {}", file.display()))?;
        Ok(Object {
            kind: Kind::Blob,
            expected_size: stat.len(),
            reader: file,
        })
    }

    /// Read a git object from `.git/objects/` using its hash.
    ///
    /// Git stores objects in `.git/objects/ab/cdef...` where `abcdef...` is the full hash.
    /// The first two characters form the subdirectory name.
    ///
    /// The object file is compressed with Zlib. After decompression, the format is:
    /// `<kind> <size>\0<content>`
    ///
    /// This function:
    /// 1. Opens the object file from `.git/objects/`
    /// 2. Decompresses it with Zlib
    /// 3. Parses the header to extract kind and size
    /// 4. Returns an Object with a reader limited to the expected size
    ///
    /// TODO: support shortest-unique object hashes
    pub(crate) fn read(hash: &str) -> anyhow::Result<Object<impl BufRead>> {
        // TODO: support shortest-unique object hashes
        // Open the object file (first 2 chars are directory, rest is filename)
        let f = std::fs::File::open(format!(".git/objects/{}/{}", &hash[..2], &hash[2..]))
            .context("open in .git/objects")?;

        // Create a Zlib decoder to decompress the object file
        let z = ZlibDecoder::new(f);
        let mut z = BufReader::new(z);

        // Read the header until we hit a null byte (0)
        // Header format: "blob 123\0" or "tree 456\0" etc.
        let mut buf = Vec::new();
        z.read_until(0, &mut buf)
            .context("read header from .git/objects")?;

        // Convert the raw bytes into a C-style null-terminated string
        // This validates that there's exactly one null byte and it's at the end
        let header = CStr::from_bytes_with_nul(&buf)
            .expect("know there is exactly one nul, and it's at the end");

        // Convert the CStr to a Rust string slice (validates UTF-8)
        let header = header
            .to_str()
            .context(".git/objects file header isn't valid UTF-8")?;

        // Split the header on the first space: "blob 123" -> ("blob", "123")
        let Some((kind, size)) = header.split_once(' ') else {
            anyhow::bail!(".git/objects file header did not start with a known type: '{header}'");
        };

        // Match the kind string to determine the object type
        let kind = match kind {
            "blob" => Kind::Blob,
            "tree" => Kind::Tree,
            "commit" => Kind::Commit,
            _ => anyhow::bail!("what even is a '{kind}'"),
        };

        // Parse the size string into an unsigned 64-bit integer
        let size = size
            .parse::<u64>()
            .context(".git/objects file header has invalid size: {size}")?;

        // Limit the reader to only read 'size' bytes
        // NOTE: this won't error if the decompressed file is too long, but will at least not
        // spam stdout and be vulnerable to a zipbomb.
        let z = z.take(size);

        Ok(Object {
            kind,
            expected_size: size,
            reader: z,
        })
    }
}

impl<R> Object<R>
where
    R: Read,
{
    /// Write the object to a writer and compute its SHA-1 hash.
    ///
    /// The object is written in git's object format:
    /// 1. Compressed with Zlib
    /// 2. Format: `<kind> <size>\0<content>`
    ///
    /// The hash is computed over the compressed data and returned.
    /// This can be used to compute the hash without writing to disk (e.g., with `std::io::sink()`).
    pub(crate) fn write(mut self, writer: impl Write) -> anyhow::Result<[u8; 20]> {
        // Create a Zlib encoder to compress the output
        let writer = ZlibEncoder::new(writer, Compression::default());

        // Wrap the writer with a HashWriter to compute SHA-1 while writing
        let mut writer = HashWriter {
            writer,
            hasher: Sha1::new(),
        };

        // Write the header: "<kind> <size>\0"
        write!(writer, "{} {}\0", self.kind, self.expected_size)?;

        // Copy the object content (reader) to the writer
        std::io::copy(&mut self.reader, &mut writer).context("stream file into blob")?;

        // Finish compression and get the final hash
        let _ = writer.writer.finish()?;
        let hash = writer.hasher.finalize();

        Ok(hash.into())
    }

    /// Write the object to `.git/objects/` and return its hash.
    ///
    /// This function:
    /// 1. Writes the object to a temporary file (computing the hash in the process)
    /// 2. Creates the appropriate subdirectory in `.git/objects/` (first 2 chars of hash)
    /// 3. Moves the temporary file to the final location: `.git/objects/ab/cdef...`
    ///
    /// The hash is used both to determine the file location and as the return value.
    pub(crate) fn write_to_objects(self) -> anyhow::Result<[u8; 20]> {
        // Write to a temporary file first (this computes the hash)
        let tmp = "temporary";
        let hash = self
            .write(std::fs::File::create(tmp).context("construct temporary file for tree")?)
            .context("stream tree object into tree object file")?;

        // Encode hash as hex string for directory/filename construction
        let hash_hex = hex::encode(hash);

        // Create the subdirectory: .git/objects/ab/ (first 2 chars of hash)
        fs::create_dir_all(format!(".git/objects/{}/", &hash_hex[..2]))
            .context("create subdir of .git/objects")?;

        // Move the temporary file to the final location: .git/objects/ab/cdef...
        fs::rename(
            tmp,
            format!(".git/objects/{}/{}", &hash_hex[..2], &hash_hex[2..]),
        )
        .context("move tree file into .git/objects")?;

        Ok(hash)
    }
}

/// A writer that computes a SHA-1 hash of all data written to it.
///
/// This wraps another writer and updates a SHA-1 hasher with every write.
/// Used when writing git objects to compute their hash while writing.
struct HashWriter<W> {
    writer: W,
    hasher: Sha1,
}

impl<W> Write for HashWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Write to the underlying writer
        let n = self.writer.write(buf)?;
        // Update the hasher with the bytes that were actually written
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}
