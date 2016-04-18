package main

import (
	"os"
	"fmt"
	"syscall"
	"math/rand"
	"time"
	"crypto/md5"
	"path"
	"strconv"
	"encoding/hex"
	"io/ioutil"
	"strings"
	"log"
	"os/signal"
)

var exit_please = false

func syntax() {
	fmt.Printf("Usage: %s \n[-h] [-vf <file> | -r <directory> |" +
			"-rc <directory> <seed> <size>]\n", os.Args[0])
	os.Exit(1)
}

func is_directory(path string) (bool) {

	if fileInfo, err := os.Stat(path); err == nil {
		return fileInfo.IsDir()
	}
	return false
}

func disk_usage(path string) (uint64, uint64) {
	var stat syscall.Statfs_t

	syscall.Statfs(path, &stat)
	free := uint64(stat.Bsize) * uint64(stat.Bavail)
	total := uint64(stat.Bsize) * uint64(stat.Blocks)
	return total, free
}

var ascii_uppercase = []byte("ABCDEFGHIJKLMNOPQRSTUVWXYZ")

func rs(seed int64, file_size uint64) ([]byte) {
	b := make([]byte, file_size)

	rand.Seed(seed)

	for i := range b {
		b[i] = ascii_uppercase[rand.Intn(len(ascii_uppercase))]
	}
	return b
}

func md5_sum(data []byte) (string) {
	sum := md5.Sum(data)
	return hex.EncodeToString(sum[:])
}

func create_file(directory string, seed int64, file_size uint64) (string, uint64) {
	total, free := disk_usage(directory)

	if file_size == 0 {
		available := uint64(float64(total) * float64(0.50))
		if free <= available {
			return "", 0
		}
		free -= available
		r_file_size := uint64(512 + rand.Intn(1024*1024*8))
		if free > r_file_size {
			file_size = r_file_size
		} else {
			file_size = free
		}
	}

	if seed == 0 {
		t_now := time.Now()
		seed = t_now.Unix()
	}

	data := rs(seed, file_size)

	file_hash := md5_sum(data)

	// Build the file name and protect it with a md5 too
	fn := fmt.Sprintf("%s-%d-%d", file_hash, seed, file_size)
	fn_hash := md5_sum([]byte(fn))
	final_name := path.Join(directory,
		fmt.Sprintf("%s:%s:integrity", fn, fn_hash))

	// Check to see if a file doesn't already exist
	if _, err := os.Stat(final_name); err == nil {
		for i := 0; i < 50; i++ {
			tmp := final_name + fmt.Sprintf(".%d", i)

			if _, err := os.Stat(tmp); os.IsNotExist(err) {
				final_name = tmp
				break
			}
		}
	}

	if _, err := os.Stat(final_name); err == nil {
		return "", 0
	}

	err := ioutil.WriteFile(final_name, data, 0744)
	if err != nil {
		panic(err)
	}

	return final_name, file_size
}

func file_size_get(full_file_name string) (int64, error) {
	if file, err := os.Open(full_file_name); err == nil {
		if fi, err := file.Stat(); err == nil {
			return fi.Size(), nil
		} else {
			return -1, err
		}
	} else {
		return -1, err
	}
}

func check(err error) {
	if err != nil {
		log.Fatal(err)
	}
}

func verify_file(full_file_name string) bool {
    // First verify the meta data is intact
    f_name := path.Base(full_file_name)
    parts := strings.Split(f_name, ":")

	name := parts[0]
	meta_hash := parts[1]
	extension := parts[2]

	if strings.HasPrefix(extension, "integrity") != true {
		fmt.Printf("File extension %s does not end in \"integrity*\"!\n",
			full_file_name)
		return false
	}

	f_hash := md5_sum([]byte(name))
	if meta_hash != f_hash {
		fmt.Printf("File %s meta data not valid! (stored = %s, calculated = %s)\n",
			full_file_name, meta_hash, f_hash)
		return false
	}

    // check file size
	parts = strings.Split(name, "-")
	file_data_hash := parts[0]
	meta_size_str := parts[2]

	meta_size, err := strconv.ParseInt(meta_size_str, 10, 64);
	check(err)

	file_size, err := file_size_get(full_file_name)
	check(err)

    if file_size != meta_size {
        fmt.Printf("File %s incorrect size! (expected = %d, current = %d)\n",
			full_file_name, meta_size, file_size)
		return false
	}

	data, err := ioutil.ReadFile(full_file_name)
	check(err)

    // Finally check the data bytes
    calculated := md5_sum(data)

    if calculated != file_data_hash {
		print("File %s md5 miss-match! (expected = %s, current = %s)",
			full_file_name, file_data_hash, calculated)
		return false
	}

    return true
}

func is_dir_or_exit(d string) {
	if false == is_directory(d) {
		fmt.Printf("%s is not a directory!\n", d)
		os.Exit(1)
	}
}

func test(directory string) {
    // Create files and random directories in the supplied directory
    var files_created  []string
    num_files_created := 0
    var total_bytes = uint64(0)

	for {
		if exit_please {
			fmt.Printf("We created %d files with a total of %d bytes!\n",
						num_files_created, total_bytes)
			os.Exit(0)
		}

		f_created, size := create_file(directory, 0, 0)

		if size > 0 {
			num_files_created += 1
			total_bytes += size
			files_created = append(files_created, f_created)
		} else {
			fmt.Printf("Full, verify and delete sequence starting...\n")
			// We don't have space, lets verify all and then
			// delete every other file
			for _, element := range files_created {
				if verify_file(element) != true {
					fmt.Printf("File %s not validating!\n", element)
					fmt.Printf("We created %d files with a total of %d bytes!\n",
						num_files_created, total_bytes)
					os.Exit(1)
				}
			}

			for i := len(files_created) - 1; i >= 0; i -= 2 {
				fn := files_created[i]
				err := os.Remove(fn)
				check(err)
				files_created = append(files_created[:i], files_created[i+1:]...)
			}
		}
	}
}

func main() {
	sigs := make(chan os.Signal, 1)
	signal.Notify(sigs, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		sig := <-sigs
		exit_please = true
		fmt.Print("\nGot signal: ", sig, "\n")
	}()


	if len(os.Args) < 2 {
		syntax()
	}

	if os.Args[1] == "-r" && len(os.Args) == 3 {
		// Run test
		d := os.Args[2]
		is_dir_or_exit(d)
		test(d)
	} else if os.Args[1] == "-vf" && len(os.Args) == 3 {
		// Verify file
		f := os.Args[2]

		if verify_file(os.Args[2]) == false {
			fmt.Printf("File %s corrupt [ERROR]!\n",  f)
            os.Exit(2)
		}
		fmt.Printf("File %s validates [OK]!\n",  f)
        os.Exit(0)

	} else if os.Args[1] == "-rc" && len(os.Args) == 5 {
		// Re-create a file
		d := os.Args[2]
		is_dir_or_exit(d)

		seed, err_seed := strconv.ParseInt(os.Args[3], 10, 64)
		file_size, err_fs := strconv.ParseUint(os.Args[4], 10, 64)

		if err_seed != nil || err_fs != nil {
			fmt.Printf("Seed or file size incorrect!\n")
			os.Exit(1)
		}

		f, _ := create_file(d, seed, file_size)
		if f != "" {
			fmt.Printf("File recreate as %s\n" , f)
			os.Exit(0)
		}
		os.Exit(1)

	} else if os.Args[1] == "-h"{
		syntax()
	} else {
		syntax()
	}
}
