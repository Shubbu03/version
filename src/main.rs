use anyhow::{bail, ensure, Context};
use clap::{Parser, Subcommand};
use flate2::read::ZlibDecoder;
use std::ffi::CStr;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufReader};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    CatFile {
        #[clap(short = 'p')]
        pretty_print: bool,

        object_hash: String,
    },
}

enum Kind {
    Blob,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Init => {
            fs::create_dir(".git").unwrap();
            fs::create_dir(".git/objects").unwrap();
            fs::create_dir(".git/refs").unwrap();
            fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
            println!("Initialised git directory")
        }
        Command::CatFile {
            pretty_print,
            object_hash,
        } => {
            ensure!(
                pretty_print,
                "mode must be given without -p, and we dont support mode"
            );
            
            // TODO: support shortest unique object hashes
            let f = File::open(format!(
                "./git/objects/{}/{}",
                &object_hash[..2],
                &object_hash[2..]
            ))
            .context("open in .git/objects")?;

            // Blob object storage in git happens inside the .git/objects dir, which contains Header and contents of the blob object compressed using Zlib. format of blob object after ZLib decompression is -
            // blob <size>\0<content> where
            // <size>  is the size of the content in bytes
            // \0 is the nul byte
            // <content> is the actual content of the file
            // the `git cat-file`  command is used to read the blob content from the .git/objects dir
            // USAGE: git cat-file -p <blob-sha>

            let z = ZlibDecoder::new(f);
            let mut z = BufReader::new(z);
            let mut buf = Vec::new();
            z.read_until(0, &mut buf)
                .context("read header from .git/objects")?;

            let header = CStr::from_bytes_with_nul(&buf)
                .expect("know there is exactly one nul and its the end");

            let header = header
                .to_str()
                .context(".git/objects file header isn't valid UTF-8")?;

            let Some((kind, size)) = header.split_once(' ') else {
                bail!(".git/objects file header did not start with 'blob' : '{header}'");
            };

            let kind = match kind {
                "blob" => Kind::Blob,
                _ => bail!("we do not know how to print a {kind}"),
            };

            let size = size
                .parse::<usize>()
                .context(".git/objects file header has invalid size : {size}")?;
            buf.clear();
            buf.resize(size, 0);
            z.read_exact(&mut buf[..])
                .context("read true contents of .git/objects file")?;
            let n = z
                .read(&mut [0])
                .context("validate EOF in .git/objects file")?;
            ensure!(n == 0, ".git/objects file had {n} trailing bytes");

            let stdout = io::stdout();
            let mut stdout = stdout.lock();

            match kind {
                Kind::Blob => stdout
                    .write_all(&buf)
                    .context("write object contents to stdout")?,
            }
        }
    }
    Ok(())
}
