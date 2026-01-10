use anyhow::{bail, ensure, Context, Result};
use flate2::read::ZlibDecoder;
// Import CStr for working with C-style null-terminated strings
use std::ffi::CStr;
use std::fs::File;
use std::io::{self, BufReader};
use std::io::{copy, prelude::*};

// Enum to represent different types of git objects
enum Kind {
    // Blob type represents file content in git
    Blob,
}

// Execute the cat-file command to display contents of a git object
pub fn execute(pretty_print: bool, object_hash: String) -> Result<()> {
    // Ensure the -p flag is provided, bail with error message if not
    ensure!(
        pretty_print,
        "mode must be given without -p, and we dont support mode"
    );

    // TODO: support shortest unique object hashes
    // Open the git object file using the hash (first 2 chars are directory, rest is filename)
    // Git stores objects in .git/objects/ab/cdef... where abcdef... is the full hash
    let f = File::open(format!(
        ".git/objects/{}/{}",
        &object_hash[..2],
        &object_hash[2..]
    ))
    // Add context to any error that occurs during file opening
    .context("open in .git/objects")?;

    // Blob object storage in git happens inside the .git/objects dir, which contains Header and contents of the blob object compressed using Zlib. format of blob object after ZLib decompression is -
    // blob <size>\0<content> where
    // <size>  is the size of the content in bytes
    // \0 is the nul byte
    // <content> is the actual content of the file
    // the `git cat-file`  command is used to read the blob content from the .git/objects dir
    // USAGE: git cat-file -p <blob-sha>

    // Create a Zlib decoder that wraps the file to decompress its contents
    let z = ZlibDecoder::new(f);
    // Wrap the decoder in a buffered reader for efficient reading
    let mut z = BufReader::new(z);
    // Create an empty vector to store bytes we read
    let mut buf = Vec::new();
    // Read bytes from the decompressed stream until we hit a null byte (0)
    // This reads the header portion: "blob <size>\0"
    z.read_until(0, &mut buf)
        // Add context if reading fails
        .context("read header from .git/objects")?;

    // Convert the raw bytes into a C-style null-terminated string
    // This validates that there's exactly one null byte and it's at the end
    let header =
        CStr::from_bytes_with_nul(&buf).expect("know there is exactly one nul and its the end");

    // Convert the CStr to a Rust string slice (&str)
    // This validates that the header is valid UTF-8
    let header = header
        .to_str()
        // Add context if the conversion fails (invalid UTF-8)
        .context(".git/objects file header isn't valid UTF-8")?;

    // Try to split the header on the first space character
    // Expected format: "blob 123" where "blob" is kind and "123" is size
    let Some((kind, size)) = header.split_once(' ') else {
        // If there's no space, the header is malformed - bail with error
        bail!(".git/objects file header did not start with 'blob' : '{header}'");
    };

    // Match the kind string to determine what type of object this is
    let kind = match kind {
        // If it's "blob", use the Blob variant
        "blob" => Kind::Blob,
        // For any other type, we don't support it yet - bail with error
        _ => bail!("we do not know how to print a {kind}"),
    };

    // Parse the size string into an unsigned 64-bit integer
    let size = size
        .parse::<u64>()
        // Add context if parsing fails (size is not a valid number)
        .context(".git/objects file header has invalid size : {size}")?;

    // NOTE: this won't error if the decompressed file is too long, but atleast will not spam stdout and be vulnerable to a zipbomp
    // Limit the reader to only read 'size' bytes
    // This prevents reading beyond the object's content (protection against zip bombs)
    let mut z = z.take(size);

    // Match on the object kind to determine how to display it
    match kind {
        Kind::Blob => {
            // Get a handle to standard output
            let stdout = io::stdout();
            // Lock stdout for exclusive access (more efficient for multiple writes)
            let mut stdout = stdout.lock();
            // Copy all bytes from the limited reader to stdout
            // Returns the number of bytes actually copied
            let n = copy(&mut z, &mut stdout).context("write .git/objects file to stdout")?;
            // Verify that the number of bytes copied matches the expected size
            // If not, the file was corrupted or truncated
            ensure!(
                n == size,
                ".git/objects file was not the expected size (expected : {size} , actual : {n})"
            );
        }
    }
    Ok(())
}
