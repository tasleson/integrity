package main

import (
	"crypto/md5"
	"encoding/hex"
	"fmt"
	"io/ioutil"
	"log"
	"math/rand"
	"os"
	"os/signal"
	"path"
	"strconv"
	"strings"
	"syscall"
	"time"
)

var exitPlease = false

func syntax() {
	fmt.Printf("Usage: %s \n[-h] [-vf <file> | -r <directory> |"+
		"-rc <directory> <seed> <size>]\n", os.Args[0])
	os.Exit(1)
}

func isDirectory(path string) bool {

	if fileInfo, err := os.Stat(path); err == nil {
		return fileInfo.IsDir()
	}
	return false
}

func diskUsage(path string) (uint64, uint64) {
	var stat syscall.Statfs_t

	syscall.Statfs(path, &stat)
	free := uint64(stat.Bsize) * uint64(stat.Bavail)
	total := uint64(stat.Bsize) * uint64(stat.Blocks)
	return total, free
}

var asciiUppercase = []byte("ABCDEFGHIJKLMNOPQRSTUVWXYZ")

func rs(seed int64, fileSize uint64) []byte {
	b := make([]byte, fileSize)

	rand.Seed(seed)

	for i := range b {
		b[i] = asciiUppercase[rand.Intn(len(asciiUppercase))]
	}
	return b
}

func md5Sum(data []byte) string {
	sum := md5.Sum(data)
	return hex.EncodeToString(sum[:])
}

func createFile(directory string, seed int64, fileSize uint64) (string, uint64) {
	total, free := diskUsage(directory)

	if fileSize == 0 {
		available := uint64(float64(total) * float64(0.50))
		if free <= available {
			return "", 0
		}
		free -= available
		randFileSize := uint64(512 + rand.Intn(1024*1024*8))
		if free > randFileSize {
			fileSize = randFileSize
		} else {
			fileSize = free
		}
	}

	if seed == 0 {
		timeNow := time.Now()
		seed = timeNow.Unix()
	}

	data := rs(seed, fileSize)

	fileHash := md5Sum(data)

	// Build the file name and protect it with a md5 too
	fn := fmt.Sprintf("%s-%d-%d", fileHash, seed, fileSize)
	fileNameHash := md5Sum([]byte(fn))
	finalName := path.Join(directory,
		fmt.Sprintf("%s:%s:integrity", fn, fileNameHash))

	// Check to see if a file doesn't already exist
	if _, err := os.Stat(finalName); err == nil {
		for i := 0; i < 50; i++ {
			tmp := finalName + fmt.Sprintf(".%d", i)

			if _, err := os.Stat(tmp); os.IsNotExist(err) {
				finalName = tmp
				break
			}
		}
	}

	if _, err := os.Stat(finalName); err == nil {
		return "", 0
	}

	f, err := os.OpenFile(finalName, os.O_RDWR|os.O_CREATE, 0744)
	if err != nil {
		panic(err)
	}

	if _, err := f.Write(data); err != nil {
		panic(err)
	}

	err = f.Sync()
	if err != nil {
		panic(err)
	}

	err = f.Close()
	if err != nil {
		panic(err)
	}

	d, err := os.OpenFile(directory, os.O_RDONLY, 0744)
	if err != nil {
		panic(err)
	}

	err = d.Sync()
	if err != nil {
		panic(err)
	}

	err = d.Close()
	if err != nil {
		panic(err)
	}

	return finalName, fileSize
}

func fileSizeGet(fullFileName string) (int64, error) {
	var file, err = os.Open(fullFileName)

	if err == nil {
		if fi, err := file.Stat(); err == nil {
			return fi.Size(), nil
		}
		return -1, err
	}
	return -1, err
}

func check(err error) {
	if err != nil {
		log.Fatal(err)
	}
}

func verifyFile(fullFileName string) bool {
	// First verify the meta data is intact
	fileName := path.Base(fullFileName)
	parts := strings.Split(fileName, ":")

	name := parts[0]
	metaHash := parts[1]
	extension := parts[2]

	if strings.HasPrefix(extension, "integrity") != true {
		fmt.Printf("File extension %s does not end in \"integrity*\"!\n",
			fullFileName)
		return false
	}

	fileHash := md5Sum([]byte(name))
	if metaHash != fileHash {
		fmt.Printf("File %s meta data not valid! (stored = %s, calculated = %s)\n",
			fullFileName, metaHash, fileHash)
		return false
	}

	// check file size
	parts = strings.Split(name, "-")
	fileDataHash := parts[0]
	metaSizeStr := parts[2]

	metaSize, err := strconv.ParseInt(metaSizeStr, 10, 64)
	check(err)

	fileSize, err := fileSizeGet(fullFileName)
	check(err)

	if fileSize != metaSize {
		fmt.Printf("File %s incorrect size! (expected = %d, current = %d)\n",
			fullFileName, metaSize, fileSize)
		return false
	}

	data, err := ioutil.ReadFile(fullFileName)
	check(err)

	// Finally check the data bytes
	calculated := md5Sum(data)

	if calculated != fileDataHash {
		print("File %s md5 miss-match! (expected = %s, current = %s)",
			fullFileName, fileDataHash, calculated)
		return false
	}

	return true
}

func isDirOrExit(d string) {
	if false == isDirectory(d) {
		fmt.Printf("%s is not a directory!\n", d)
		os.Exit(1)
	}
}

func test(directory string) {
	// Create files and random directories in the supplied directory
	var filesCreated []string
	numFilesCreated := 0
	var totalBytes = uint64(0)

	for {
		if exitPlease {
			fmt.Printf("We created %d files with a total of %d bytes!\n",
				numFilesCreated, totalBytes)
			os.Exit(0)
		}

		fileCreateSize, size := createFile(directory, 0, 0)

		if size > 0 {
			numFilesCreated++
			totalBytes += size
			filesCreated = append(filesCreated, fileCreateSize)
		} else {
			fmt.Printf("Full, verify and delete sequence starting...\n")
			// We don't have space, lets verify all and then
			// delete every other file
			for _, element := range filesCreated {
				if verifyFile(element) != true {
					fmt.Printf("File %s not validating!\n", element)
					fmt.Printf("We created %d files with a total of %d bytes!\n",
						numFilesCreated, totalBytes)
					os.Exit(1)
				}
			}

			for i := len(filesCreated) - 1; i >= 0; i -= 2 {
				fn := filesCreated[i]
				err := os.Remove(fn)
				check(err)
				filesCreated = append(filesCreated[:i], filesCreated[i+1:]...)
			}
		}
	}
}

func main() {
	sigs := make(chan os.Signal, 1)
	signal.Notify(sigs, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		sig := <-sigs
		exitPlease = true
		fmt.Print("\nGot signal: ", sig, "\n")
	}()

	if len(os.Args) < 2 {
		syntax()
	}

	if os.Args[1] == "-r" && len(os.Args) == 3 {
		// Run test
		d := os.Args[2]
		isDirOrExit(d)
		test(d)
	} else if os.Args[1] == "-vf" && len(os.Args) == 3 {
		// Verify file
		f := os.Args[2]

		if verifyFile(os.Args[2]) == false {
			fmt.Printf("File %s corrupt [ERROR]!\n", f)
			os.Exit(2)
		}
		fmt.Printf("File %s validates [OK]!\n", f)
		os.Exit(0)

	} else if os.Args[1] == "-rc" && len(os.Args) == 5 {
		// Re-create a file
		d := os.Args[2]
		isDirOrExit(d)

		seed, errSeed := strconv.ParseInt(os.Args[3], 10, 64)
		fileSize, errFs := strconv.ParseUint(os.Args[4], 10, 64)

		if errSeed != nil || errFs != nil {
			fmt.Printf("Seed or file size incorrect!\n")
			os.Exit(1)
		}

		f, _ := createFile(d, seed, fileSize)
		if f != "" {
			fmt.Printf("File recreate as %s\n", f)
			os.Exit(0)
		}
		os.Exit(1)

	} else if os.Args[1] == "-h" {
		syntax()
	} else {
		syntax()
	}
}
