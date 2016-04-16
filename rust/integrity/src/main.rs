extern crate rand;
extern crate time;
extern crate crypto;
extern crate nix;

use rand::{Rng, SeedableRng, StdRng};
use rand::distributions::{IndependentSample, Range};
use crypto::md5::Md5;
use crypto::digest::Digest;
use nix::sys::statfs::statfs;
use nix::sys::statfs::vfs::Statfs;
use std::cmp;
use std::path::Path;
use std::path::PathBuf;
use std::fs;
use std::fs::metadata;
use std::fs::File;
use std::io::prelude::*;
use std::vec::Vec;
use std::process::exit;
use std::env;
use nix::sys::signal;

static mut exit_please: bool = false;

extern fn handle_sigint(_:i32) {
    unsafe {
        exit_please = true;
    }
}

fn disk_usage(path : &str) -> (u64, u64) {
    let mut fs = Statfs {f_bavail: 0, f_bfree: 0, f_type: 0, f_frsize: 0,
                         f_ffree: 0, f_namelen: 0, f_fsid: 0, f_blocks: 0,
                         f_files: 0, f_spare: [0,0,0,0,0], f_bsize: 0};
    statfs(path, &mut fs).unwrap();
    let free = (fs.f_bsize as u64 * fs.f_bfree) as u64;
    let total = (fs.f_bsize as u64 * fs.f_blocks) as u64;
    (total, free)
}

fn rs(seed: usize, file_size: usize) -> String {
    let s: &[_] = &[seed];
    let mut rng: StdRng = SeedableRng::from_seed(s);
    rng.gen_ascii_chars().take(file_size).collect()
}

fn md5_sum(data: &str) -> String {
    let mut hasher = Md5::new();
    hasher.input_str(data);
    hasher.result_str()
}

fn get_file_size(path: &str) -> i64 {
    match metadata(path) {
        Ok(n) => n.len() as i64,
        Err(_) => -1,
    }
}

fn is_directory(path: &str) -> bool {
    match metadata(path) {
        Ok(n) => n.is_dir(),
        Err(_) => false,
    }
}

fn file_exists(full_file_name: &str) -> bool {
    match metadata(full_file_name) {
        Ok(n) => return n.is_file(),
        Err(_) => return false,
    }
}

fn run(directory: &str) {
    let mut files_created = Vec::new();
    let mut num_files_created = 0;
    let mut total_bytes: u64 = 0;
    let mut l_exit = false;

    while l_exit == false {
        let (f_created, size) = create_file(directory, 0, 0);

        if size > 0 {
            num_files_created += 1;
            total_bytes += size as u64;
            files_created.push(f_created.clone());
        } else {
            println!("Full, verify and delete sequence starting...");

            // Walk the list, verifying every file
            for f in &files_created {
                if !verify_file(f) {
                    println!("File {} not validating!", f);
		    println!("We created {} files with a total of {} bytes!",
			     num_files_created, total_bytes);
                    exit(1);
                }
            }

            // Delete every other file
            let count = files_created.len();
            for i in (0..count).rev().filter(|&x| x % 2 == 0) {
                let file_to_delete = files_created.remove(i);
                //println!("Deleting file {}", file_to_delete);
                let error_msg = format!("Error: Unable to remove file {}!",
                                        file_to_delete);
                fs::remove_file(file_to_delete).expect(&error_msg);
            }
        }

        unsafe {
            l_exit = exit_please;
        }
    }
    println!("We created {} files with a total of {} bytes!",
	     num_files_created, total_bytes);
}

fn create_file(directory: &str, seed: usize, file_size: usize) -> (String, usize) {
    let mut l_file_size = file_size;
    let mut l_seed = seed;
    let mut tmp_name: String;
    let (f_total, f_free) = disk_usage(directory);

    if l_file_size == 0 {
        let between = Range::new(512, 1024*1024*8);
        let mut rng = rand::thread_rng();
        let available = (f_total as f64 * 0.5) as u64;

        if f_free <= available {
            return (String::from(""), 0)
        }

        l_file_size = (f_free - available) as usize;
        let tmp_file_size = between.ind_sample(&mut rng);
        l_file_size = cmp::min(l_file_size, tmp_file_size);
    }

    if l_seed == 0 {
        l_seed  = time::get_time().sec as usize;
    }

    let data = rs(l_seed, l_file_size);
    let file_hash = md5_sum(&data[..]);

    //Build the file name
    let file_name = format!("{}-{}-{}", file_hash, l_seed, l_file_size);
    let file_name_hash = md5_sum(&file_name[..]);

    //Build full file name and path
    let mut final_name = PathBuf::from(directory);
    final_name.push(format!("{}:{}:integrity", file_name, file_name_hash));
    let mut final_name_str = final_name.to_str().unwrap();

    // Ensure the file we are wanting to create doesn't exist, if it does
    // we will append and try again
    if file_exists(final_name_str) {
        for x in 0..50 {
            tmp_name = format!("{}.{}", final_name_str, x);
            if !file_exists(&tmp_name[..]) {
                final_name_str = &tmp_name;
                break;
            }
        }
    }

    if file_exists(final_name_str) {
        return (String::from(""), 0);
    }

    let f = File::create(final_name_str);
    if f.is_ok() {
        f.unwrap().write_all(data.as_bytes()).expect("Shorted write?");
    } else {
        println!("Unable to create file {}!", final_name_str);
        return (String::from(""), 0);
    }

    (String::from(final_name_str), l_file_size)
}

fn verify_file(full_file_name: &str) -> bool {
    // First verify the meta data is intact
    let f_name = Path::new(full_file_name).file_name().unwrap().to_str().unwrap();
    let parts = f_name.split(":").collect::<Vec<&str>>();

    let name = parts[0];
    let meta_hash = parts[1];
    let extension = parts[2];

    // Check extension
    if extension.starts_with("integrity") != true {
        println!("File extension {} does not end in \"integrity*\"!",
                 full_file_name);
        return false;
    }

    // Check metadata
    let f_hash = md5_sum(name);
    if meta_hash != f_hash {
	println!("File {} meta data not valid! (stored = {}, calculated = {})",
		 full_file_name, meta_hash, f_hash);
	return false;
    }

    let name_parts = name.split("-").collect::<Vec<&str>>();
    let file_data_hash = name_parts[0];
    let meta_size = name_parts[2].parse::<i64>().unwrap();
    let file_size = get_file_size(full_file_name);

    if meta_size != file_size {
        println!("File {} incorrect size! (expected = {}, current = {})\n",
	         full_file_name, meta_size, file_size);
	return false;
    }

    // Read in the data
    let mut data = String::new();

    let f = File::open(full_file_name);
    if f.is_ok() {
        f.unwrap().read_to_string(&mut data).expect("Shorted read?");
    } else {
        // Without using a match, how do we get the err info?
        println!("Unable to read file {}!", full_file_name);
        return false;
    }

    // Generate the md5
    let calculated = md5_sum(&data);

    // Compare md5
    if file_data_hash != calculated {
        println!("File {} md5 miss-match! (expected = {}, current = {})",
		 full_file_name, file_data_hash, calculated);
	return false;
    }

    true
}

fn syntax() {
    let prg = &env::args().nth(0).unwrap();
    println!("Usage: {} \n[-h] [-vf <file> | -r <directory> |-rc  \
              <directory> <seed> <size>]\n", prg);
    exit(1);
}

fn main() {
    let args: Vec<_> = env::args().collect();

    if args.len() < 2 {
	syntax();
    }

    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigint),
        signal::SaFlag::empty(),
        signal::SigSet::empty());
    unsafe {
        signal::sigaction(signal::SIGINT, &sig_action).
            expect("Unable to install signal handler!");
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
	let f = &args[2];

	if verify_file(f) == false {
	    println!("File {} corrupt [ERROR]!\n",  f);
            exit(2);
	}
	println!("File {} validates [OK]!\n",  f);
        exit(0);

    } else if args[1] == "-rc" && args.len() == 5 {
	// Re-create a file
	let d = &args[2];

        if is_directory(d) == false {
            println!("{} is not a directory!", d);
            exit(1);
        }

        let seed = args[3].parse::<usize>().unwrap();
        let file_size = args[4].parse::<usize>().unwrap();

	let (f, _) = create_file(d, seed, file_size);
	if f != "" {
	    println!("File recreated as {}" , f);
	    exit(0);
	}
	exit(1);

    } else if args[1] == "-h"{
	syntax();
    } else {
	syntax();
    }
}
