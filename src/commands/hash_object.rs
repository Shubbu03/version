use crate::objects::Object;
use anyhow::Context;
use std::path::Path;

/// Execute the hash-object command to compute the SHA-1 hash of a file.
/// 
/// If `write` is true, the blob object is written to `.git/objects/` directory.
/// If `write` is false, only the hash is computed and printed (without writing to disk).
/// 
/// The function uses the `Object` abstraction to create a blob from the file,
/// then either writes it to the objects directory or computes the hash by writing to a sink.
pub fn execute(write: bool, file: &Path) -> anyhow::Result<()> {
    // Create a blob object from the file using the Object abstraction
    let object = Object::blob_from_file(file).context("open blob input file")?;
    
    // Compute the hash, optionally writing to .git/objects/
    let hash = if write {
        // Write the blob object to .git/objects/ and return its hash
        object
            .write_to_objects()
            .context("stream file into blob object file")?
    } else {
        // Compute hash by writing to a sink (no disk I/O)
        object
            .write(std::io::sink())
            .context("stream file into blob object")?
    };

    // Print the hash as a hexadecimal string
    println!("{}", hex::encode(hash));

    Ok(())
}
