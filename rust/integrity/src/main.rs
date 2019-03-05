extern crate crypto;
extern crate nix;
extern crate rand;
extern crate time;

use crypto::digest::Digest;
use crypto::md5::Md5;
use nix::sys::signal;
use nix::sys::statvfs::statvfs;
use rand::distributions::Alphanumeric;
use rand::rngs::StdRng;
use rand::{thread_rng, Rng, SeedableRng};
use std::cmp;
use std::env;
use std::fs;
use std::fs::metadata;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::iter;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;
use std::vec::Vec;

static mut EXIT_PLEASE: bool = false;

extern "C" fn handle_sigint(_: i32) {
    unsafe {
        EXIT_PLEASE = true;
    }
}

fn disk_usage(path: &str) -> (u64, u64) {
    let fs = statvfs(path).unwrap();

    let free = (fs.block_size() as u64 * fs.blocks_free()) as u64;
    let total = (fs.block_size() as u64 * fs.blocks()) as u64;
    (total, free)
}

fn rs(seed: u64, file_size: usize) -> String {
    let mut rng: StdRng = SeedableRng::seed_from_u64(seed);

    iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .take(file_size)
        .collect()
}

fn md5_sum(data: &str) -> String {
    let mut hasher = Md5::new();
    hasher.input_str(data);
    hasher.result_str()
}

fn is_directory(path: &str) -> bool {
    match metadata(path) {
        Ok(n) => n.is_dir(),
        Err(_) => false,
    }
}

fn run(directory: &str) {
    let mut files_created = Vec::new();
    let mut num_files_created = 0;
    let mut total_bytes: u64 = 0;
    let mut l_exit = false;

    while !l_exit {
        match create_file(directory, None, None) {
            Ok((f_created, size)) => {
                num_files_created += 1;
                total_bytes += size as u64;
                files_created.push(f_created);
            }
            Err(_) => {
                println!("Full, verify and delete sequence starting...");

                // Walk the list, verifying every file
                for f in &files_created {
                    if verify_file(f).is_err() {
                        println!("File {} not validating!", f.display());
                        println!(
                            "We created {} files with a total of {} bytes!",
                            num_files_created, total_bytes
                        );
                        exit(1);
                    }
                }

                // Delete every other file
                let count = files_created.len();
                for i in (0..count).rev().filter(|&x| x % 2 == 0) {
                    let file_to_delete = files_created.remove(i);
                    //println!("Deleting file {}", file_to_delete);
                    let error_msg =
                        format!("Error: Unable to remove file {}!", file_to_delete.display());
                    fs::remove_file(file_to_delete).expect(&error_msg);
                }
            }
        }

        unsafe {
            l_exit = EXIT_PLEASE;
        }
    }
    println!(
        "We created {} files with a total of {} bytes!",
        num_files_created, total_bytes
    );
}

fn create_file(
    directory: &str,
    seed: Option<u64>,
    file_size: Option<usize>,
) -> io::Result<(PathBuf, u64)> {
    let (f_total, f_free) = disk_usage(directory);

    let l_file_size = match file_size {
        Some(size) => size,
        None => {
            let available = (f_total as f64 * 0.5) as u64;

            if f_free <= available {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("f_free {} <= available {}", f_free, available),
                ));
            }

            let mut rng = thread_rng();
            let tmp_file_size = rng.gen_range(512, 1024 * 1024 * 8);
            cmp::min((f_free - available) as usize, tmp_file_size)
        }
    };

    let l_seed = match seed {
        Some(seed) => seed,
        None => time::get_time().sec as u64,
    };

    let data = rs(l_seed, l_file_size);
    let file_hash = md5_sum(&data[..]);

    //Build the file name
    let file_name = format!("{}-{}-{}", file_hash, l_seed, l_file_size);
    let file_name_hash = md5_sum(&file_name[..]);

    //Build full file name and path
    let mut final_name = PathBuf::from(directory);
    final_name.push(format!("{}:{}:integrity", file_name, file_name_hash));

    let final_name = {
        if !final_name.exists() {
            final_name
        } else {
            match (0..50)
                .map(|num| final_name.with_file_name(format!("{}.{}", final_name.display(), num)))
                .find(|pathbuf| !pathbuf.exists())
            {
                Some(x) => x,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "Could not generate unique name for {}",
                            final_name.display()
                        ),
                    ))
                }
            }
        }
    };

    let mut f = File::create(&final_name)?;
    f.write_all(data.as_bytes())?;
    f.sync_all()?;

    Ok((final_name, l_file_size as u64))
}

fn verify_file(full_file_name: &Path) -> io::Result<()> {
    // First verify the meta data is intact
    let f_name = Path::new(full_file_name)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let parts = f_name.split(':').collect::<Vec<&str>>();

    let name = parts[0];
    let meta_hash = parts[1];
    let extension = parts[2];

    // Check extension
    if extension.starts_with("integrity") != true {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "File extension {} does not end in \"integrity*\"!",
                full_file_name.display()
            ),
        ));
    }

    // Check metadata
    let f_hash = md5_sum(name);
    if meta_hash != f_hash {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "File {} meta data not valid! (stored = {}, calculated = {})",
                full_file_name.display(),
                meta_hash,
                f_hash
            ),
        ));
    }

    let name_parts = name.split('-').collect::<Vec<&str>>();
    let file_data_hash = name_parts[0];
    let meta_size = name_parts[2].parse::<u64>().unwrap();
    let file_size = metadata(full_file_name)?.len();

    if meta_size != file_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "File {} incorrect size! (expected = {}, current = {})\n",
                full_file_name.display(),
                meta_size,
                file_size
            ),
        ));
    }

    // Read in the data
    let mut data = String::new();

    let mut f = File::open(full_file_name)?;
    f.read_to_string(&mut data).expect("Shorted read?");

    // Generate the md5
    let calculated = md5_sum(&data);

    // Compare md5
    if file_data_hash != calculated {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "File {} md5 miss-match! (expected = {}, current = {})",
                full_file_name.display(),
                file_data_hash,
                calculated
            ),
        ));
    }

    Ok(())
}

fn syntax() {
    let prg = &env::args().nth(0).unwrap();
    println!(
        "Usage: {} \n[-h] [-vf <file> | -r <directory> |-rc  \
         <directory> <seed> <size>]\n",
        prg
    );
    exit(1);
}

fn main() {
    let args: Vec<_> = env::args().collect();

    if args.len() < 2 {
        syntax();
    }

    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigint),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe {
        signal::sigaction(signal::SIGINT, &sig_action).expect("Unable to install signal handler!");
    }

    if args[1] == "-r" && args.len() == 3 {
        // Run test
        let d = &args[2];
        if is_directory(d) {
            run(d);
        } else {
            println!("{} is not a directory!", d);
            exit(1);
        }
    } else if args[1] == "-vf" && args.len() == 3 {
        // Verify file
        let f = PathBuf::from(&args[2]);

        if verify_file(&f).is_err() {
            println!("File {} corrupt [ERROR]!\n", f.display());
            exit(2);
        }
        println!("File {} validates [OK]!\n", f.display());
        exit(0);
    } else if args[1] == "-rc" && args.len() == 5 {
        // Re-create a file
        let d = &args[2];

        if !is_directory(d) {
            println!("{} is not a directory!", d);
            exit(1);
        }

        let seed = args[3].parse::<u64>().unwrap();
        let file_size = args[4].parse::<usize>().unwrap();

        if let Ok((f, _)) = create_file(d, Some(seed), Some(file_size)) {
            println!("File recreated as {}", f.display());
            exit(0);
        }
        exit(1);
    } else {
        syntax();
    }
}
