# NeoDOS VFS Improvements (v0.5)

## Problem Statement

The original NeoDOS filesystem had critical VFS issues:

1. **No Directory Validation**: The `CD` command allowed entering non-existent directories without error
2. **Global File Search**: All file searches (`find_file()`, `TYPE`, `COPY`) only looked in root, not in subdirectories
3. **Floating Files**: Files appeared to exist everywhere - same listing regardless of current directory
4. **Broken `CD ..`**: Parent directory navigation didn't work correctly

### Example of Broken Behavior
```
C:\> dir
  readme.txt
  test.bat
  SYSTEM

C:\> cd hola           <- Should fail: directory doesn't exist!
C:\hola> dir           <- But lists same files as root
  readme.txt
  test.bat
  SYSTEM

C:\hola> cd ..
C:\> dir
  readme.txt
  test.bat
  SYSTEM
```

## Solution: Proper VFS Layer

### New Functions in `neodos_fs.rs`

#### 1. `find_entry_in_directory(dir_inode_num, filename, ...) -> Result<(u32, u8), FsError>`
- Searches for a file/directory within any inode (not just root)
- Returns the inode number and entry type
- **This is the core VFS function** that all other lookups depend on

```rust
pub fn find_entry_in_directory(&mut self, dir_inode_num: u32, filename: &str, 
                                cache: &mut BlockCache, ata: &mut AtaDriver) 
    -> Result<(u32, u8), FsError>
```

#### 2. `find_file_in_directory(dir_inode_num, filename, ...) -> Result<u32, FsError>`
- Searches for a **file** in a specific directory
- Validates that the entry is a file (MODE_FILE)
- Used by `TYPE` and `COPY` commands

#### 3. `find_directory_in_directory(dir_inode_num, dirname, ...) -> Result<u32, FsError>`
- Searches for a **directory** in a specific directory
- Validates that the entry is a directory (MODE_DIR)
- **Used by `CD` command for validation**

#### 4. Updated `find_file(filename, ...)` 
- Backward compatible wrapper
- Now uses `find_entry_in_directory(0, ...)` internally (searches in root)
- For AUTOEXEC.BAT and other root-level files

### New Functions in `shell.rs`

#### `navigate_to_path(path: &str) -> Result<u32, FsError>`
- Parses a full path (e.g., `\SYSTEM\CONFIG`) from root
- Traverses directory tree returning the final inode
- Used by `CD ..` to navigate back to parent
- Splits path by backslashes and navigates through each component

### Updated Commands in `commands.rs`

#### 1. `cmd_cd()` - Now Validates Directories
**Before**: Blindly added path without checking
```rust
// TODO: Validate directory exists and get its inode
```

**After**: Validates using `find_directory_in_directory()`
```rust
match self.fs.find_directory_in_directory(self.current_dir_inode, path, ...) {
    Ok(new_inode) => {
        // Valid directory - update current_dir_inode
        self.current_dir_inode = new_inode;
    }
    Err(_) => println!("The system cannot find the path specified"),
}
```

#### 2. `cmd_type()` - Searches Current Directory
**Before**: Always searched root only
```rust
self.fs.find_file(filename, ...) // Only looks in root
```

**After**: Searches current directory
```rust
self.fs.find_file_in_directory(self.current_dir_inode, filename, ...)
```

#### 3. `cmd_call()` - Searches Current Directory for BAT Files
Same change as `cmd_type()` - now searches in `current_dir_inode`

#### 4. `cmd_copy()` - Searches Current Directory for Source
**Before**: Source file always searched in root
**After**: Searches in current directory
```rust
self.fs.find_file_in_directory(self.current_dir_inode, src, ...)
```

## Architecture

### Directory Traversal Flow
```
cmd_cd("hola")
  ↓
find_directory_in_directory(current_inode, "hola", ...)
  ↓
find_entry_in_directory(current_inode, "hola", ...)
  ↓
Reads directory's blocks, searches for entry by name
  ↓
Validates entry is MODE_DIR
  ↓
Updates current_dir_inode = returned_inode
```

### File Lookup Flow (for TYPE command)
```
cmd_type("file.txt")
  ↓
find_file_in_directory(current_inode, "file.txt", ...)
  ↓
find_entry_in_directory(current_inode, "file.txt", ...)
  ↓
Searches in current directory's blocks
  ↓
Validates entry is MODE_FILE
  ↓
Returns inode_num for reading
```

## Testing the Fix

### Test 1: Directory Validation
```bash
C:\> cd nonexistent
The system cannot find the path specified

C:\> cd SYSTEM        # SYSTEM exists
C:\SYSTEM> dir
  config.sys
  drivers
C:\SYSTEM>
```

### Test 2: File Isolation Per Directory
```bash
C:\> copy readme.txt system_readme.txt
C:\> cd SYSTEM
C:\SYSTEM> type readme.txt
File not found              # Correct! readme.txt is in root, not here
C:\SYSTEM> type config.sys
[displays config.sys contents]
```

### Test 3: Parent Navigation
```bash
C:\> cd SYSTEM
C:\SYSTEM> cd ..
C:\>                        # Correctly returns to root
```

## Implementation Details

- **Block structure preserved**: Still uses same inode/directory entry format
- **No breaking changes**: Backward compatible with existing `find_file()` calls
- **VFS-aware**: All file operations now respect directory context
- **Proper error handling**: Returns `NotADirectory` or `NotAFile` as appropriate

## Performance Considerations

- Each lookup traverses directory blocks sequentially (no hashing)
- Multiple lookups in same directory = redundant I/O (acceptable for NeoDOS scope)
- Cache layer still handles sector caching

## Future Improvements

1. Directory entry caching for faster repeated lookups
2. Support for absolute paths (e.g., `TYPE \SYSTEM\CONFIG.SYS`)
3. Wildcard support in directory operations
4. More robust path parsing (handle edge cases)
5. Case-insensitive filename matching
