use jif::*;

use clap::Parser;
use std::fs::File;
use std::io::BufReader;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    jif_file: std::path::PathBuf,

    #[arg(long)]
    raw: bool,
}

fn func() -> JifResult<()> {
    let args = Cli::parse();
    let mut file = BufReader::new(File::open(&args.jif_file)?);

    if args.raw {
        let jif = JifRaw::from_reader(&mut file)?;
        println!("{:#x?}", jif);
    } else {
        let jif = Jif::from_reader(&mut file)?;
        println!("{:#x?}", jif);
    }
    Ok(())
}

fn main() {
    if let Err(e) = func() {
        eprintln!("{}", e);
    }
}
