 # integrity

Simple utility to generate files in directory and validate that they are still correct.

```
Usage: integrity [-h] [-vf <file> | -r <directory> |-rc <directory> <seed> <size>]

-h   Show help message
-r <run dir> Create files until the free space ~50%, then validate every file and
             delete every other file before continuing to create more files
-vf <file>   Validate file
-rc <dir> <seed> <size>  Re-create a file given the seed and size 
```
Files created have the name:
```
375335f3d1f494b15746636d869e2fd7-100-1024:3939474373090544562f0f2124e7470c:integrity
<data md5 sum>-<seed>-<size in bytes>:<file name md5sum>:<extension>[.N]?
```

The tool is implemented in a number of different languages as a learning experience and to compare/contrast strengths and weaknesses of each.
