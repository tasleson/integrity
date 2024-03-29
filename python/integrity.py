#!/bin/env python3
#
# Theory of operation:
# randomly create files until you get the FS 50% full
# then verify all the files and then start removing/recreating files while
# verifying them.
#
# Each file name will be in the form:
# <file md5>-<random seed>-<file size>:<meta md5>:integrity format.
# Thus given the file name we will be able to tell what the file md5 sum is, the
# random seed used to create the random sequence and verify that the metadata is
# correct with the file meta md5.

import random
import hashlib
import os
import sys
import argparse
import string
import datetime

cs = list(string.ascii_uppercase + string.ascii_lowercase + string.digits)

QUIT_ON_FULL = False
DUPLICATE = False
DUPLICATE_DATA = None
MAX_FILE_SIZE = 1024*1024*8
SEED = 0
PERCENT_FREE = 0.50
BLOCK_SIZE = 512
FILE_COUNT = 0


def rs(str_len):
    """
    Generate a random string
    """
    global DUPLICATE
    global DUPLICATE_DATA

    if DUPLICATE:
        if DUPLICATE_DATA is None:
            DUPLICATE_DATA = ''.join([cs[int(random.random() * len(cs))] for _ in range(MAX_FILE_SIZE)])
        return DUPLICATE_DATA[0:str_len]
    return ''.join([cs[int(random.random() * len(cs))] for _ in range(str_len)])


def disk_usage(path):
    st = os.statvfs(path)
    free = st.f_bavail * st.f_frsize
    total = st.f_blocks * st.f_frsize
    return total, free


def md5(t):
    h = hashlib.md5()
    h.update(t.encode("utf-8"))
    return h.hexdigest()


def _round_to_block_size(size):
    return size if size % BLOCK_SIZE == 0 else size + BLOCK_SIZE - size % BLOCK_SIZE


def create_file(directory, seed=0, file_size=0):
    total, free = disk_usage(directory)

    if file_size == 0:
        # Don't fill to capacity
        if free <= int(total * PERCENT_FREE):
            return None, 0
        free -= int(total * PERCENT_FREE)

        r_file_size = random.randint(BLOCK_SIZE, MAX_FILE_SIZE)
        file_size = min(free, r_file_size)

        # Make the file size more easily de-duped
        if DUPLICATE:
            file_size = _round_to_block_size(file_size)

    data = rs(file_size)

    file_hash = md5(data)

    # Build the file name and protect it with a md5 too
    fn = "%s-%d-%d" % (file_hash, seed, file_size)
    fn_hash = md5(fn)
    final_name = os.path.join(directory, "%s:%s:integrity" % (fn, fn_hash))

    # Check to make sure file doesn't already exist
    if os.path.exists(final_name):
        i = 0
        while True:
            tmp = final_name + ".%d" % i
            if not os.path.exists(tmp):
                final_name = tmp
                break
            i += 1

    if os.path.exists(final_name):
        return "", 0

    with open(final_name, 'w') as out:
        out.write(data)
        out.flush()
        os.fsync(out.fileno())

    fd = os.open(directory, os.O_RDONLY)
    os.fsync(fd)
    os.close(fd)

    return final_name, file_size


def verify_file(full_file_name):
    # First verify the metadata is intact
    f_name = os.path.basename(full_file_name)
    name, meta_hash, extension = f_name.split(':')

    if not extension.startswith('integrity'):
        print('File extension %s does not end in "integrity*"!' %
              full_file_name)
        return False

    f_hash = md5(name)
    if meta_hash != f_hash:
        print("File %s meta data not valid! (stored = %s, calculated = %s)" %
              (full_file_name, meta_hash, f_hash))
        return False

    # check file size
    file_data_hash, _, file_size = name.split('-')
    if os.path.getsize(full_file_name) != int(file_size):
        print("File %s incorrect size! (expected = %d, current = %d)" %
              (full_file_name, file_size, os.path.getsize(full_file_name)))
        return False

    # Finally check the data bytes
    h = hashlib.md5()

    with open(full_file_name, 'r') as in_file:
        d = in_file.read(4096)
        while d:
            h.update(d.encode("utf-8"))
            d = in_file.read(4096)

    calculated = h.hexdigest()
    if calculated != file_data_hash:
        print("File %s md5 miss-match! (expected = %s, current = %s)" %
              (full_file_name, file_data_hash, calculated))
        return False

    return True


def test(directory, seed):
    # Create files and random directories in the supplied directory
    files_created = []
    num_files_created = 0
    total_bytes = 0
    current_count = 0

    try:
        while FILE_COUNT == 0 or current_count < FILE_COUNT:
            f_created, size = create_file(directory, seed=seed)
            if f_created:
                num_files_created += 1
                current_count = current_count + 1 if FILE_COUNT != 0 else 0
                total_bytes += size
                files_created.append(f_created)
            else:
                if QUIT_ON_FULL:
                    print("exiting on full request")
                    sys.exit(0)
                print('Full, verify and delete sequence starting...')
                # We don't have space, lets verify all and then
                # delete every other file
                for t in files_created:
                    if not verify_file(t):
                        print("File %s not validating!")
                        print("We created %s files with a total of %s bytes!" %
                              (str(num_files_created), str(total_bytes)))
                        sys.exit(1)

                num = len(files_created)
                for i in range(num-1, -1, -2):
                    fn = files_created[i]
                    os.remove(fn)
                    del files_created[i]

        print("Exiting on number of file limit reached")

    except KeyboardInterrupt:
        print("Exiting: We created %s files with a total of %s bytes!" %
              (str(num_files_created), str(total_bytes)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group()
    group.add_argument('-vf', '--verify-file', action="store",
                       dest="verify_files", nargs="+",
                       type=str, help="File(s) to verify", default="")
    group.add_argument('-r', '--run', action="store", dest="run_dir",
                       type=str, help="Directory to run test in", default="")
    group.add_argument('-rc', '--recreate', nargs=3,
                       action="store", dest="recreate_args", default=None,
                       help="Recreate a file given a <directory> <seed> <size>")
    parser.add_argument('-qf', '--quit-on-full', action="store_true", dest="quit_on_full",
                        default=False, help="Exit when you fill up FS to 'percent_free'")
    parser.add_argument('-dup', '--duplicate', action="store_true", dest="duplicate",
                        default=False,
                        help="Create files which contain data that is similar")
    parser.add_argument('-s', '--seed', dest="seed", default=0, action="store", type=int,
                        help="Test run overall seed, allows you to recreate the exact same sequence")

    group2 = parser.add_mutually_exclusive_group()
    group2.add_argument('-pf', '--percent_free', dest="percent_free", default=0.50, action="store", type=float,
                        help="What percent free should remain on FS before we start the delete sequence, or exit")
    group2.add_argument('-c', '--count', dest="file_count", default=0, action="store", type=int,
                        help="Number of files to create before optionally exiting, or starting delete sequence")

    args = parser.parse_args()

    QUIT_ON_FULL = args.quit_on_full
    DUPLICATE = args.duplicate
    SEED = args.seed if args.seed != 0 else int(datetime.datetime.now().microsecond)
    random.seed(SEED)
    PERCENT_FREE = args.percent_free
    FILE_COUNT = args.file_count

    if args.run_dir:
        if os.path.isdir(args.run_dir):
            test(args.run_dir, SEED)
        else:
            print("%s is not a directory!" % args.run_dir)
            sys.exit(1)
    elif args.verify_files:
        for f in args.verify_files:
            if not verify_file(f):
                print('File %s corrupt [ERROR]!' % f)
                sys.exit(2)
            print('File %s validates [OK]!' % f)
        sys.exit(0)
    elif args.recreate_args:
        if SEED != 0:
            print("-s|--seed should not be specified with -rc!")
            sys.exit(1)
        random.seed(args.recreate_args[1])
        f = create_file(
                args.recreate_args[0],
                int(args.recreate_args[1]),
                int(args.recreate_args[2]))[0]
        if f:
            print("File recreated as %s" % f)
            sys.exit(0)
        sys.exit(1)
    else:
        parser.print_help()
        sys.exit(1)
