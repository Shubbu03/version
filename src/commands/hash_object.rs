use anyhow::{Context, Result};
use flate2::{write::ZlibEncoder, Compression};
use sha1::{Digest, Sha1};
use std::fs::{create_dir_all, metadata, rename, File};
use std::io::{copy, prelude::*};
use std::path::{Path, PathBuf};

pub fn execute(write: bool, file: PathBuf) -> Result<()> {
    let hash = if write {
        let tmp = "temporary";
        let hash = write_blob(
            &file,
            File::create(tmp).context("construct temp file for blob object")?,
        )
        .context("write out blob object")?;
        create_dir_all(format!(".git/objects/{}/", &hash[..2]))
            .context("create sub dir of .git/objects")?;
        rename(tmp, format!(".git/objects/{}/{}", &hash[..2], &hash[2..]))
            .context("mpve blob file into .git/objects")?;
        hash
    } else {
        write_blob(&file, std::io::sink()).context("write out blob object")?
    };

    println!("{hash}");

    Ok(())
}

struct HashWriter<W> {
    writer: W,
    hasher: Sha1,
}

impl<W> Write for HashWriter<W>
where
    W: Write,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.writer.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

fn write_blob<W>(file: &Path, writer: W) -> Result<String>
where
    W: Write,
{
    let stat = metadata(&file).with_context(|| format!("stat {}", file.display()))?;

    let writer = ZlibEncoder::new(writer, Compression::default());

    let mut writer = HashWriter {
        writer,
        hasher: Sha1::new(),
    };

    write!(writer, "blob ")?;
    write!(writer, "{}\0", stat.len())?;
    let mut file = File::open(&file).with_context(|| format!("open {}", file.display()))?;
    copy(&mut file, &mut writer).context("stream file into blob")?;
    let _ = writer.writer.finish()?;
    let hash = writer.hasher.finalize();

    Ok(hex::encode(hash))
}
