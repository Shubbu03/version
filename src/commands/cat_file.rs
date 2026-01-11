use crate::objects::{Kind, Object};
use anyhow::Context;

/// Execute the cat-file command to display contents of a git object.
/// 
/// Blob object storage in git happens inside the .git/objects dir, which contains
/// Header and contents of the blob object compressed using Zlib. Format of blob
/// object after ZLib decompression is:
///   blob <size>\0<content>
/// where:
///   <size>  is the size of the content in bytes
///   \0      is the nul byte
///   <content> is the actual content of the file
/// 
/// The `git cat-file` command is used to read the blob content from the .git/objects dir
/// USAGE: git cat-file -p <blob-sha>
/// 
/// Git stores objects in .git/objects/ab/cdef... where abcdef... is the full hash.
/// The first two characters form the subdirectory name.
/// 
/// The function reads a git object from `.git/objects/` using the object hash,
/// decompresses it, parses the header, and writes the content to stdout.
/// Currently only supports blob objects. The object is read using the `Object::read`
/// method which handles decompression, header parsing, and size validation.
pub fn execute(pretty_print: bool, object_hash: &str) -> anyhow::Result<()> {
    // Ensure the -p flag is provided, bail with error message if not
    anyhow::ensure!(
        pretty_print,
        "mode must be given without -p, and we don't support mode"
    );

    // Read the object from .git/objects/ using the Object abstraction
    // This handles:
    // - Opening the git object file using the hash (first 2 chars are directory, rest is filename)
    // - Creating a Zlib decoder that wraps the file to decompress its contents
    // - Wrapping the decoder in a buffered reader for efficient reading
    // - Reading bytes from the decompressed stream until we hit a null byte (0)
    //   This reads the header portion: "blob <size>\0"
    // - Converting the raw bytes into a C-style null-terminated string
    //   This validates that there's exactly one null byte and it's at the end
    // - Converting the CStr to a Rust string slice (&str) - validates UTF-8
    // - Splitting the header on the first space character
    //   Expected format: "blob 123" where "blob" is kind and "123" is size
    // - Matching the kind string to determine what type of object this is
    // - Parsing the size string into an unsigned 64-bit integer
    // - Setting up a reader limited to the expected size
    //   NOTE: this won't error if the decompressed file is too long, but at least will not
    //   spam stdout and be vulnerable to a zipbomb. This prevents reading beyond the
    //   object's content (protection against zip bombs)
    let mut object = Object::read(object_hash).context("parse out blob object file")?;
    
    // Match on the object kind to determine how to display it
    match object.kind {
        Kind::Blob => {
            // Get a handle to standard output
            // Lock stdout for exclusive access (more efficient for multiple writes)
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            
            // Copy all bytes from the limited reader to stdout
            // Returns the number of bytes actually copied
            // The reader is already limited to expected_size bytes (protection against zip bombs)
            let n = std::io::copy(&mut object.reader, &mut stdout)
                .context("write .git/objects file to stdout")?;
            
            // Verify that the number of bytes copied matches the expected size
            // If not, the file was corrupted or truncated
            anyhow::ensure!(
                n == object.expected_size,
                ".git/object file was not the expected size (expected: {}, actual: {n})",
                object.expected_size
            );
        }
        // For any other object type, we don't support it yet - bail with error
        _ => anyhow::bail!("don't yet know how to print '{}'", object.kind),
    }

    Ok(())
}