#!/usr/bin/env python3
import struct
import sys
import os

BLOCK_SIZE = 4096
SECTOR_SIZE = 512
SUPERBLOCK_MAGIC = 0x4F444F4E  # "NEOD"
NUM_INODES = 256
DATA_START_SECTOR = 1 + (NUM_INODES * 256 + 511) // 512  # = 129 for 256 inodes
ROOT_DIR_BLOCK = 0

# NeoDOS permission and mode flags (matching neodos_fs.rs)
MODE_DIR = 0x0040
MODE_FILE = 0x0080
PERM_R = 0x0001
PERM_W = 0x0002
PERM_X = 0x0004
PERM_S = 0x0008
PERM_D = 0x0010


def default_perms_for_filename(name):
    """Match kernel's NeodosFs::default_perms_for_filename() logic."""
    upper = name.upper()
    if upper.endswith('.NXE') or upper.endswith('.COM') or upper.endswith('.EXE'):
        return PERM_R | PERM_X
    elif upper.endswith('.NEM'):
        return PERM_R
    elif upper.endswith('.NXL'):
        return PERM_R | PERM_X
    elif upper.endswith('.BAT') or upper.endswith('.CMD'):
        return PERM_R | PERM_X
    elif upper.endswith('.SYS'):
        return PERM_R
    elif upper.endswith('.CFG') or upper.endswith('.INI'):
        return PERM_R | PERM_W
    elif upper.endswith('.TXT') or upper.endswith('.MD') or upper.endswith('.LOG') or upper.endswith('.ASC'):
        return PERM_R | PERM_W
    else:
        return PERM_R | PERM_W

def create_superblock(volume_label=""):
    """Crear superbloque (512 bytes)"""
    data = bytearray(512)
    
    # Magic
    data[0:4] = struct.pack('<I', SUPERBLOCK_MAGIC)
    
    # Block size
    data[4:8] = struct.pack('<I', BLOCK_SIZE)
    
    # Num blocks (10 MB / 4KB = 2560 blocks)
    data[8:12] = struct.pack('<I', 2560)
    
    # Num inodes (max 256)
    data[12:16] = struct.pack('<I', 256)
    
    # Created timestamp
    data[16:24] = struct.pack('<Q', 0)
    
    # Volume label
    label_bytes = volume_label.encode('utf-8')[:11]
    data[24] = len(label_bytes)
    data[25:25+len(label_bytes)] = label_bytes
    # Fill rest with spaces
    for i in range(25+len(label_bytes), 36):
        data[i] = ord(' ')
    
    return bytes(data)

def create_inode(inode_num, mode, size, direct_blocks):
    """Crear inode (256 bytes)"""
    data = bytearray(256)
    
    # Inode number
    data[0:4] = struct.pack('<I', inode_num)
    
    # Mode
    data[4:6] = struct.pack('<H', mode)
    
    # Size
    data[6:10] = struct.pack('<I', size)
    
    # Times (atime, mtime, ctime)
    data[10:18] = struct.pack('<Q', 0)  # atime
    data[18:26] = struct.pack('<Q', 0)  # mtime
    data[26:34] = struct.pack('<Q', 0)  # ctime
    
    # Link count
    data[34:36] = struct.pack('<H', 1)
    
    # UID/GID
    data[36:40] = struct.pack('<I', 0)  # uid
    data[40:44] = struct.pack('<I', 0)  # gid
    
    # Direct blocks (12 × u32)
    for i, block in enumerate(direct_blocks):
        data[44 + i*4:44 + (i+1)*4] = struct.pack('<I', block)
    
    # Indirect block (0 for now)
    data[44+12*4:44+12*4+4] = struct.pack('<I', 0)
    
    return bytes(data)

def create_dir_entry(inode_num, entry_type, name, attributes=0):
    """Crear directory entry (256 bytes)"""
    data = bytearray(256)
    
    # Inode number
    data[0:4] = struct.pack('<I', inode_num)
    
    # Name length
    data[4] = len(name)
    
    # Entry type (1=file, 2=dir)
    data[5] = entry_type
    
    # Attributes (default: Archive for files, Dir for dirs)
    if attributes == 0:
        attributes = 0x10 if entry_type == 2 else 0x20  # DIR or ARCHIVE
    data[6] = attributes
    
    # Name (starts at offset 7)
    data[7:7+len(name)] = name.encode('utf-8')
    
    return bytes(data)


class InodeAllocator:
    def __init__(self):
        self._next = 1
        self._allocated = set()

    def alloc(self, name=""):
        """Allocate the next free inode number."""
        while self._next in self._allocated:
            self._next += 1
            if self._next >= 256:
                raise RuntimeError("Inode table full (max 256)")
        inum = self._next
        self._allocated.add(inum)
        self._next += 1
        return inum

    def reserve(self, inum, name=""):
        """Reserve a specific inode number (for root=0)."""
        if inum in self._allocated:
            raise RuntimeError(f"Inode {inum} already allocated for {name}")
        self._allocated.add(inum)
        if inum >= self._next:
            self._next = inum + 1
        return inum

    def check_no_duplicates(self):
        """Final validation - no duplicates means len(_allocated) equals max assigned."""
        pass


def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--label', default='NEO-DISK')
    parser.add_argument('--output', default='neodos_image.img')
    parser.add_argument('--minimal', action='store_true',
        help='Create minimal image with only test.txt')
    parser.add_argument('--readme', default='''Welcome to NeoDOS!
This is a DOS-like operating system.
Built with NeoDOS FS v1.0 (inodes, not FAT).
Cluster: Block 1, Inode 1, Size: 1024 bytes.
Happy hacking!
''')
    args = parser.parse_args()
    vol_label = args.label[:11]
    readme_text = args.readme

    # Crear imagen vacía (10 MB)
    image_size = 10 * 1024 * 1024
    image = bytearray(image_size)
    
    # 1. Superbloque @ sector 0
    print(f"[*] Writing superblock (label={vol_label})...")
    superblock = create_superblock(vol_label)
    image[0:512] = superblock
    
    # 2. Inode table @ sectors 1-63 (125 inodes max)
    print("[*] Writing inode table...")

    # .nem test driver files
    nem_dir = os.environ.get('NEM_DIR', '/tmp/nem_drivers_0')

    if args.minimal:
        # --- Minimal disk: only test.txt ---
        allocator = InodeAllocator()
        allocator.reserve(0, "root")
        inode_test = allocator.alloc("test.txt")
        dir_mode = MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D
        root_inode = create_inode(0, dir_mode, 256, [ROOT_DIR_BLOCK, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512:512+256] = root_inode
        txt_inode = create_inode(inode_test, MODE_FILE | default_perms_for_filename("test.txt"), 56, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+256:512+512] = txt_inode

        # Root dir
        offset = (DATA_START_SECTOR + ROOT_DIR_BLOCK * 8) * 512
        entry = create_dir_entry(inode_test, 1, "test.txt")
        image[offset:offset+256] = entry

        # Data block 1
        offset = (DATA_START_SECTOR + 8) * 512
        txt_content = b"This is the secondary disk (D:). Only has this file.\r\n"
        image[offset:offset+len(txt_content)] = txt_content
    else:
        dir_mode = MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D

        base_path = os.path.join(os.path.dirname(__file__), "..", "preferences")

        def read_cfg(name):
            with open(os.path.join(base_path, name), "rb") as f:
                return f.read()

        bootcfg_content = read_cfg("boot.cfg")
        system_cfg_content = read_cfg("system.cfg")
        input_cfg_content = read_cfg("input.cfg")
        es_nkb_content = b"[NeoDOS Keyboard Layout]\r\nName=es-ES\r\nDescription=Spanish (Spain)\r\n"
        en_nkb_content = b"[NeoDOS Keyboard Layout]\r\nName=en-US\r\nDescription=English (United States)\r\n"

        # ── Dynamic inode allocator (early allocations) ──
        allocator = InodeAllocator()
        allocator.reserve(0, "root")
        inode_readme = allocator.alloc("readme.txt")
        inode_testbat = allocator.alloc("test.bat")

        # Inodes: root dir, readme.txt, test.bat
        root_inode = create_inode(0, dir_mode, BLOCK_SIZE, [ROOT_DIR_BLOCK, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512:512+256] = root_inode
        readme_inode = create_inode(inode_readme, MODE_FILE | default_perms_for_filename("readme.txt"), 1024, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+256:512+512] = readme_inode
        testbat_inode = create_inode(inode_testbat, MODE_FILE | default_perms_for_filename("test.bat"), 512, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+512:512+768] = testbat_inode

        # ── Read binary data ──
        userbin_dir = os.path.join(os.path.dirname(__file__), '..', 'userbin')
        nxe_files = {}
        for name in ['cpuinfo', 'neoshell', 'neoinit', 'coredir', 'cd', 'corehelp', 'datetime', 'ver', 'neomem', 'vol', 'echo', 'label', 'coretype', 'tree', 'corecls', 'corecopy', 'coredel', 'coreren', 'coremd', 'corerd', 'cmdtest', 'drives', 'ps', 'keyb', 'kill', 'pri', 'fsck', 'ndreg', 'loadnem', 'progress', 'neotop']:
            fpath = os.path.join(userbin_dir, f'{name}.nxe')
            data = b''
            if os.path.exists(fpath):
                with open(fpath, 'rb') as f:
                    data = f.read()
                print(f"[*] Including {name}.nxe ({len(data)} bytes)")
            else:
                print(f"[!] {name}.nxe not found — skipping")
            nxe_files[name] = data



        nem_dir = os.environ.get('NEM_DIR', '/tmp/nem_drivers_0')
        def read_nem(subdir, fname):
            fpath = os.path.join(nem_dir, subdir, fname)
            if os.path.exists(fpath):
                with open(fpath, 'rb') as nf:
                    return nf.read()
            return b''

        nxl_candidates = [
            os.path.join(os.path.dirname(__file__), '..', 'libneodos.nxl'),
            os.path.join(os.path.dirname(__file__), '..', 'libneodos-nxl', 'target', 'x86_64-unknown-none', 'release', 'libneodos-nxl'),
        ]
        nxl_path = next((path for path in nxl_candidates if os.path.exists(path)), None)
        nxl_data = b''
        if nxl_path is not None:
            with open(nxl_path, 'rb') as f:
                nxl_data = f.read()
            print(f"[*] Including libneodos.nxl from {os.path.relpath(nxl_path, os.path.dirname(__file__))} ({len(nxl_data)} bytes)")
        else:
            print(f"[!] libneodos.nxl not found — NXL not included")

        math_nxl_candidates = [
            os.path.join(os.path.dirname(__file__), '..', 'libmath.nxl'),
            os.path.join(os.path.dirname(__file__), '..', 'libmath-nxl', 'target', 'x86_64-unknown-none', 'release', 'libmath-nxl'),
        ]
        math_nxl_path = next((path for path in math_nxl_candidates if os.path.exists(path)), None)
        math_nxl_data = b''
        if math_nxl_path is not None:
            with open(math_nxl_path, 'rb') as f:
                math_nxl_data = f.read()
            print(f"[*] Including libmath.nxl from {os.path.relpath(math_nxl_path, os.path.dirname(__file__))} ({len(math_nxl_data)} bytes)")
        else:
            print(f"[!] libmath.nxl not found — NXL not included")

        console_nxl_candidates = [
            os.path.join(os.path.dirname(__file__), '..', 'console.nxl'),
            os.path.join(os.path.dirname(__file__), '..', 'libconsole-nxl', 'target', 'x86_64-unknown-none', 'release', 'libconsole-nxl'),
        ]
        console_nxl_path = next((path for path in console_nxl_candidates if os.path.exists(path)), None)
        console_nxl_data = b''
        if console_nxl_path is not None:
            with open(console_nxl_path, 'rb') as f:
                console_nxl_data = f.read()
            print(f"[*] Including console.nxl from {os.path.relpath(console_nxl_path, os.path.dirname(__file__))} ({len(console_nxl_data)} bytes)")
        else:
            print(f"[!] console.nxl not found — NXL not included")

        # ── Dynamic inode allocator (remaining allocations) ──
        inode_system = allocator.alloc("System")
        inode_kernel = allocator.alloc("Kernel")
        inode_bootcfg = allocator.alloc("boot.cfg")
        inode_drivers = allocator.alloc("Drivers")
        inode_ps2kbd = allocator.alloc("ps2kbd.nem")
        inode_serial = allocator.alloc("serial.nem")
        inode_rtc = allocator.alloc("rtc.nem")
        inode_acpi = allocator.alloc("acpi.nem")
        inode_pci = allocator.alloc("pci.nem")
        inode_ata = allocator.alloc("ata.nem")
        inode_ahci = allocator.alloc("ahci.nem")
        inode_e1000 = allocator.alloc("e1000.nem")
        inode_ps2mouse = allocator.alloc("ps2mouse.nem")
        inode_virtio_blk = allocator.alloc("virtio-blk.nem")
        inode_libraries = allocator.alloc("Libraries")
        inode_fsnxl = allocator.alloc("fs.nxl")
        inode_mathnxl = allocator.alloc("math.nxl")
        inode_consolenxl = allocator.alloc("console.nxl")
        inode_layouts = allocator.alloc("Layouts")
        inode_eses = allocator.alloc("es-ES.nkb")
        inode_enus = allocator.alloc("en-US.nkb")
        inode_config = allocator.alloc("Config")
        inode_systemcfg = allocator.alloc("system.cfg")
        inode_inputcfg = allocator.alloc("input.cfg")

        # Programs
        inode_programs = allocator.alloc("Programs")
        inode_neoshell = allocator.alloc("neoshell.nxe")
        inode_neoinit = allocator.alloc("neoinit.nxe")
        inode_cpuinfo = allocator.alloc("cpuinfo.nxe")
        inode_coredir = allocator.alloc("dir.nxe")
        inode_corehelp = allocator.alloc("help.nxe")
        inode_datetime = allocator.alloc("datetime.nxe")
        inode_ver = allocator.alloc("ver.nxe")
        inode_neomem = allocator.alloc("neomem.nxe")
        inode_vol = allocator.alloc("vol.nxe")
        inode_echo = allocator.alloc("echo.nxe")
        inode_cd = allocator.alloc("cd.nxe")
        inode_coretype = allocator.alloc("type.nxe")
        inode_tree = allocator.alloc("tree.nxe")
        inode_corecls = allocator.alloc("cls.nxe")
        inode_corecopy = allocator.alloc("copy.nxe")
        inode_coredel = allocator.alloc("del.nxe")
        inode_coreren = allocator.alloc("ren.nxe")
        inode_coremd = allocator.alloc("md.nxe")
        inode_corerd = allocator.alloc("rd.nxe")
        inode_cmdtest = allocator.alloc("cmdtest.nxe")
        inode_drives_nxe = allocator.alloc("drives.nxe")
        inode_ps = allocator.alloc("ps.nxe")
        inode_keyb = allocator.alloc("keyb.nxe")
        inode_kill = allocator.alloc("kill.nxe")
        inode_pri = allocator.alloc("pri.nxe")
        inode_label = allocator.alloc("label.nxe")
        inode_fsck = allocator.alloc("fsck.nxe")
        inode_ndreg = allocator.alloc("ndreg.nxe")
        inode_loadnem = allocator.alloc("loadnem.nxe")
        inode_progress = allocator.alloc("progress.nxe")
        inode_neotop = allocator.alloc("neotop.nxe")

        # Other directories
        inode_packages = allocator.alloc("Packages")
        inode_users = allocator.alloc("Users")
        inode_default = allocator.alloc("Default")
        inode_alejandro = allocator.alloc("Alejandro")
        inode_temp = allocator.alloc("Temp")
        inode_data = allocator.alloc("Data")
        inode_logs = allocator.alloc("Logs")

        # ── Dynamic block allocator ──
        next_block = 6
        block_allocs = {}

        def alloc_blocks(inode_num, data_size):
            nonlocal next_block
            if data_size == 0:
                block_allocs[inode_num] = []
                return []
            blocks_needed = max(1, (data_size + BLOCK_SIZE - 1) // BLOCK_SIZE)
            if blocks_needed > 12:
                print(f"[!] Warning: {inode_num} needs {blocks_needed} blocks (max 12)")
                blocks_needed = 12
            blks = list(range(next_block, next_block + blocks_needed))
            next_block += blocks_needed
            block_allocs[inode_num] = blks
            return blks

        def pad_blocks(blks):
            return (blks + [0] * 12)[:12]

        # Read NEM driver data (using dynamic inodes)
        nem_data = {
            inode_ps2kbd:  ("ps2kbd.nem", read_nem("BOOT", "ps2kbd.nem")),
            inode_serial:  ("serial.nem",   read_nem("BOOT", "serial.nem")),
            inode_rtc:     ("rtc.nem",      read_nem("BOOT", "rtc.nem")),
            inode_acpi:    ("acpi.nem",     read_nem("SYSTEM", "acpi.nem")),
            inode_pci:     ("pci.nem",      read_nem("SYSTEM", "pci.nem")),
            inode_ata:     ("ata.nem",     read_nem("SYSTEM", "ata.nem")),
            inode_ahci:    ("ahci.nem",     read_nem("SYSTEM", "ahci.nem")),
            inode_e1000:   ("e1000.nem",    read_nem("SYSTEM", "e1000.nem")),
            inode_ps2mouse:("ps2mouse.nem", read_nem("BOOT", "ps2mouse.nem")),
            inode_virtio_blk:("virtio-blk.nem", read_nem("SYSTEM", "virtio-blk.nem")),
        }

        # Allocate blocks for everything
        cpuinfo_blocks    = alloc_blocks(inode_cpuinfo, len(nxe_files['cpuinfo']))
        neoshell_blocks   = alloc_blocks(inode_neoshell, len(nxe_files['neoshell']))
        neoinit_blocks    = alloc_blocks(inode_neoinit, len(nxe_files['neoinit']))
        coredir_blocks    = alloc_blocks(inode_coredir, len(nxe_files['coredir']))
        cd_blocks         = alloc_blocks(inode_cd, len(nxe_files['cd']))
        corehelp_blocks   = alloc_blocks(inode_corehelp, len(nxe_files['corehelp']))
        datetime_blocks   = alloc_blocks(inode_datetime, len(nxe_files['datetime']))
        ver_blocks        = alloc_blocks(inode_ver, len(nxe_files['ver']))
        neomem_blocks     = alloc_blocks(inode_neomem, len(nxe_files['neomem']))
        vol_blocks        = alloc_blocks(inode_vol, len(nxe_files['vol']))
        echo_blocks       = alloc_blocks(inode_echo, len(nxe_files['echo']))
        coretype_blocks   = alloc_blocks(inode_coretype, len(nxe_files['coretype']))
        tree_blocks       = alloc_blocks(inode_tree, len(nxe_files['tree']))
        corecls_blocks    = alloc_blocks(inode_corecls, len(nxe_files['corecls']))
        corecopy_blocks   = alloc_blocks(inode_corecopy, len(nxe_files['corecopy']))
        coredel_blocks    = alloc_blocks(inode_coredel, len(nxe_files['coredel']))
        coreren_blocks    = alloc_blocks(inode_coreren, len(nxe_files['coreren']))
        coremd_blocks     = alloc_blocks(inode_coremd, len(nxe_files['coremd']))
        corerd_blocks     = alloc_blocks(inode_corerd, len(nxe_files['corerd']))
        cmdtest_blocks    = alloc_blocks(inode_cmdtest, len(nxe_files['cmdtest']))
        drives_blocks     = alloc_blocks(inode_drives_nxe, len(nxe_files['drives']))
        ps_blocks         = alloc_blocks(inode_ps, len(nxe_files['ps']))
        keyb_blocks       = alloc_blocks(inode_keyb, len(nxe_files['keyb']))
        kill_blocks       = alloc_blocks(inode_kill, len(nxe_files['kill']))
        pri_blocks        = alloc_blocks(inode_pri, len(nxe_files['pri']))
        label_blocks      = alloc_blocks(inode_label, len(nxe_files['label']))
        fsck_blocks       = alloc_blocks(inode_fsck, len(nxe_files['fsck']))
        ndreg_blocks      = alloc_blocks(inode_ndreg, len(nxe_files['ndreg']))
        loadnem_blocks    = alloc_blocks(inode_loadnem, len(nxe_files['loadnem']))
        progress_blocks   = alloc_blocks(inode_progress, len(nxe_files['progress']))
        neotop_blocks     = alloc_blocks(inode_neotop, len(nxe_files['neotop']))
        fs_nxl_blocks     = alloc_blocks(inode_fsnxl, len(nxl_data))
        math_nxl_blocks   = alloc_blocks(inode_mathnxl, len(math_nxl_data))
        console_nxl_blocks = alloc_blocks(inode_consolenxl, len(console_nxl_data))
        bootcfg_blocks    = alloc_blocks(inode_bootcfg, len(bootcfg_content))
        system_cfg_blocks = alloc_blocks(inode_systemcfg, len(system_cfg_content))
        input_cfg_blocks  = alloc_blocks(inode_inputcfg, len(input_cfg_content))
        es_nkb_blocks     = alloc_blocks(inode_eses, len(es_nkb_content))
        en_nkb_blocks     = alloc_blocks(inode_enus, len(en_nkb_content))

        nem_blocks = {}
        for inum in nem_data:
            nem_blocks[inum] = alloc_blocks(inum, len(nem_data[inum][1]))

        # Directory blocks (fixed block allocation)
        sys_dir_blocks    = alloc_blocks(inode_system, 2304)    # System dir (9 entries × 256)
        kernel_dir_blocks = alloc_blocks(inode_kernel, 512)     # Kernel dir
        drv_dir_blocks    = alloc_blocks(inode_drivers, 2560)    # Drivers dir (10 entries)
        lib_dir_blocks    = alloc_blocks(inode_libraries, 1536)   # Libraries dir (5 entries)
        lay_dir_blocks    = alloc_blocks(inode_layouts, 768)    # Layouts dir (2 entries + padding)
        cfg_dir_blocks    = alloc_blocks(inode_config, 768)    # Config dir (2 entries + padding)
        prog_dir_blocks   = alloc_blocks(inode_programs, 8192)   # Programs dir (32 entries)
        pkg_dir_blocks    = alloc_blocks(inode_packages, 256)    # Packages dir (empty)
        usr_dir_blocks    = alloc_blocks(inode_users, 768)    # Users dir
        def_dir_blocks    = alloc_blocks(inode_default, 256)    # Users\Default (empty)
        ale_dir_blocks    = alloc_blocks(inode_alejandro, 256)    # Users\Alejandro (empty)
        tmp_dir_blocks    = alloc_blocks(inode_temp, 256)    # Temp dir (empty)
        dat_dir_blocks    = alloc_blocks(inode_data, 256)    # Data dir (empty)
        log_dir_blocks    = alloc_blocks(inode_logs, 256)    # Logs dir (empty)

        # ── Write inode table ──
        inodes_data = {
            inode_system:  (dir_mode, 2304, pad_blocks(sys_dir_blocks)),
            inode_kernel:  (dir_mode, 512,  pad_blocks(kernel_dir_blocks)),
            inode_bootcfg: (MODE_FILE | default_perms_for_filename("boot.cfg"), len(bootcfg_content), pad_blocks(bootcfg_blocks)),
            inode_drivers: (dir_mode, 2560, pad_blocks(drv_dir_blocks)),
            inode_libraries: (dir_mode, 1536, pad_blocks(lib_dir_blocks)),
            inode_fsnxl:   (MODE_FILE | default_perms_for_filename("fs.nxl"), len(nxl_data), pad_blocks(fs_nxl_blocks)),
            inode_mathnxl: (MODE_FILE | default_perms_for_filename("math.nxl"), len(math_nxl_data), pad_blocks(math_nxl_blocks)),
            inode_consolenxl: (MODE_FILE | default_perms_for_filename("console.nxl"), len(console_nxl_data), pad_blocks(console_nxl_blocks)),
            inode_layouts: (dir_mode, 768,  pad_blocks(lay_dir_blocks)),
            inode_eses:    (MODE_FILE | default_perms_for_filename("es-ES.nkb"), len(es_nkb_content), pad_blocks(es_nkb_blocks)),
            inode_enus:    (MODE_FILE | default_perms_for_filename("en-US.nkb"), len(en_nkb_content), pad_blocks(en_nkb_blocks)),
            inode_config:  (dir_mode, 768,  pad_blocks(cfg_dir_blocks)),
            inode_systemcfg: (MODE_FILE | default_perms_for_filename("system.cfg"), len(system_cfg_content), pad_blocks(system_cfg_blocks)),
            inode_inputcfg: (MODE_FILE | default_perms_for_filename("input.cfg"), len(input_cfg_content), pad_blocks(input_cfg_blocks)),
            inode_programs: (dir_mode, 6656, pad_blocks(prog_dir_blocks)),
            inode_neoshell: (MODE_FILE | default_perms_for_filename("NeoShell.nxe"), len(nxe_files['neoshell']), pad_blocks(neoshell_blocks)),
            inode_neoinit: (MODE_FILE | default_perms_for_filename("NeoInit.nxe"), len(nxe_files['neoinit']), pad_blocks(neoinit_blocks)),
            inode_cpuinfo: (MODE_FILE | default_perms_for_filename("cpuinfo.nxe"), len(nxe_files['cpuinfo']), pad_blocks(cpuinfo_blocks)),
            inode_coredir: (MODE_FILE | default_perms_for_filename("dir.nxe"), len(nxe_files['coredir']), pad_blocks(coredir_blocks)),
            inode_cd:      (MODE_FILE | default_perms_for_filename("cd.nxe"), len(nxe_files['cd']), pad_blocks(cd_blocks)),
            inode_corehelp: (MODE_FILE | default_perms_for_filename("help.nxe"), len(nxe_files['corehelp']), pad_blocks(corehelp_blocks)),
            inode_datetime: (MODE_FILE | default_perms_for_filename("datetime.nxe"), len(nxe_files['datetime']), pad_blocks(datetime_blocks)),
            inode_ver:     (MODE_FILE | default_perms_for_filename("ver.nxe"), len(nxe_files['ver']), pad_blocks(ver_blocks)),
            inode_neomem:  (MODE_FILE | default_perms_for_filename("neomem.nxe"), len(nxe_files['neomem']), pad_blocks(neomem_blocks)),
            inode_vol:     (MODE_FILE | default_perms_for_filename("vol.nxe"), len(nxe_files['vol']), pad_blocks(vol_blocks)),
            inode_echo:    (MODE_FILE | default_perms_for_filename("echo.nxe"), len(nxe_files['echo']), pad_blocks(echo_blocks)),
            inode_coretype: (MODE_FILE | default_perms_for_filename("type.nxe"), len(nxe_files['coretype']), pad_blocks(coretype_blocks)),
            inode_tree:    (MODE_FILE | default_perms_for_filename("tree.nxe"), len(nxe_files['tree']), pad_blocks(tree_blocks)),
            inode_corecls: (MODE_FILE | default_perms_for_filename("cls.nxe"), len(nxe_files['corecls']), pad_blocks(corecls_blocks)),
            inode_corecopy: (MODE_FILE | default_perms_for_filename("copy.nxe"), len(nxe_files['corecopy']), pad_blocks(corecopy_blocks)),
            inode_coredel: (MODE_FILE | default_perms_for_filename("del.nxe"), len(nxe_files['coredel']), pad_blocks(coredel_blocks)),
            inode_coreren: (MODE_FILE | default_perms_for_filename("ren.nxe"), len(nxe_files['coreren']), pad_blocks(coreren_blocks)),
            inode_coremd:  (MODE_FILE | default_perms_for_filename("md.nxe"), len(nxe_files['coremd']), pad_blocks(coremd_blocks)),
            inode_corerd:  (MODE_FILE | default_perms_for_filename("rd.nxe"), len(nxe_files['corerd']), pad_blocks(corerd_blocks)),
            inode_cmdtest: (MODE_FILE | default_perms_for_filename("cmdtest.nxe"), len(nxe_files['cmdtest']), pad_blocks(cmdtest_blocks)),
            inode_drives_nxe: (MODE_FILE | default_perms_for_filename("drives.nxe"), len(nxe_files['drives']), pad_blocks(drives_blocks)),
            inode_packages: (dir_mode, 256,  pad_blocks(pkg_dir_blocks)),
            inode_users:   (dir_mode, 768,  pad_blocks(usr_dir_blocks)),
            inode_default: (dir_mode, 256,  pad_blocks(def_dir_blocks)),
            inode_alejandro: (dir_mode, 256,  pad_blocks(ale_dir_blocks)),
            inode_ps:      (MODE_FILE | default_perms_for_filename("ps.nxe"), len(nxe_files['ps']), pad_blocks(ps_blocks)),
            inode_keyb:    (MODE_FILE | default_perms_for_filename("keyb.nxe"), len(nxe_files['keyb']), pad_blocks(keyb_blocks)),
            inode_kill:    (MODE_FILE | default_perms_for_filename("kill.nxe"), len(nxe_files['kill']), pad_blocks(kill_blocks)),
            inode_pri:     (MODE_FILE | default_perms_for_filename("pri.nxe"), len(nxe_files['pri']), pad_blocks(pri_blocks)),
            inode_label:   (MODE_FILE | default_perms_for_filename("label.nxe"), len(nxe_files['label']), pad_blocks(label_blocks)),
            inode_fsck:    (MODE_FILE | default_perms_for_filename("fsck.nxe"), len(nxe_files['fsck']), pad_blocks(fsck_blocks)),
            inode_ndreg:   (MODE_FILE | default_perms_for_filename("ndreg.nxe"), len(nxe_files['ndreg']), pad_blocks(ndreg_blocks)),
            inode_loadnem: (MODE_FILE | default_perms_for_filename("loadnem.nxe"), len(nxe_files['loadnem']), pad_blocks(loadnem_blocks)),
            inode_progress: (MODE_FILE | default_perms_for_filename("progress.nxe"), len(nxe_files['progress']), pad_blocks(progress_blocks)),
            inode_neotop:  (MODE_FILE | default_perms_for_filename("neotop.nxe"), len(nxe_files['neotop']), pad_blocks(neotop_blocks)),
            inode_temp:    (dir_mode, 256,  pad_blocks(tmp_dir_blocks)),
            inode_data:    (dir_mode, 256,  pad_blocks(dat_dir_blocks)),
            inode_logs:    (dir_mode, 256,  pad_blocks(log_dir_blocks)),
        }

        for inum, (mode, size, blks) in inodes_data.items():
            inode = create_inode(inum, mode, size, blks)
            offset = 512 + inum * 256
            image[offset:offset+256] = inode

        # NEM driver inodes
        for inum, (fname, fdata) in nem_data.items():
            blks = nem_blocks.get(inum, [])
            inode = create_inode(inum, MODE_FILE | default_perms_for_filename(fname), len(fdata), pad_blocks(blks))
            offset = 512 + inum * 256
            image[offset:offset+256] = inode

        # ── Root directory entries ──
        print("[*] Writing root directory...")
        offset = (DATA_START_SECTOR + ROOT_DIR_BLOCK * 8) * 512
        image[offset:offset+256]   = create_dir_entry(inode_readme, 1, "readme.txt")
        image[offset+256:offset+512] = create_dir_entry(inode_testbat, 1, "test.bat")
        image[offset+512:offset+768] = create_dir_entry(inode_system, 2, "System")
        image[offset+768:offset+1024] = create_dir_entry(inode_programs, 2, "Programs")
        image[offset+1024:offset+1280] = create_dir_entry(inode_packages, 2, "Packages")
        image[offset+1280:offset+1536] = create_dir_entry(inode_users, 2, "Users")
        image[offset+1536:offset+1792] = create_dir_entry(inode_temp, 2, "Temp")
        image[offset+1792:offset+2048] = create_dir_entry(inode_data, 2, "Data")
        image[offset+2048:offset+2304] = create_dir_entry(inode_logs, 2, "Logs")

        # ── Data blocks ──
        # Block 1 = readme.txt
        print("[*] Writing readme.txt content...")
        offset = (DATA_START_SECTOR + 8) * 512
        image[offset:offset+len(readme_text)] = readme_text.encode('utf-8')

        # Block 2 = test.bat
        print("[*] Writing test.bat content...")
        offset = (DATA_START_SECTOR + 16) * 512
        testbat_content = read_cfg("test.bat")
        image[offset:offset+len(testbat_content)] = testbat_content

        # ── System\ directory ──
        print("[*] Writing System directory...")
        blk = sys_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256]      = create_dir_entry(inode_kernel, 2, "Kernel")
        image[offset+256:offset+512]  = create_dir_entry(inode_drivers, 2, "Drivers")
        image[offset+512:offset+768]  = create_dir_entry(inode_libraries, 2, "Libraries")
        image[offset+768:offset+1024] = create_dir_entry(inode_layouts, 2, "Layouts")
        image[offset+1024:offset+1280]= create_dir_entry(inode_config, 2, "Config")

        # System\Kernel\
        print("[*] Writing System\\Kernel directory...")
        blk = kernel_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256] = create_dir_entry(inode_bootcfg, 1, "boot.cfg")

        # System\Kernel\boot.cfg content
        print("[*] Writing boot.cfg...")
        for bi, blk in enumerate(bootcfg_blocks):
            chunk = bootcfg_content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
            off = (DATA_START_SECTOR + blk * 8) * 512
            image[off:off+len(chunk)] = chunk

        # System\Drivers\ directory
        print("[*] Writing System\\Drivers directory...")
        blk = drv_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256]    = create_dir_entry(inode_ps2kbd, 1, "kbps2.nem")
        image[offset+256:offset+512]= create_dir_entry(inode_serial, 1, "serial.nem")
        image[offset+512:offset+768]= create_dir_entry(inode_rtc, 1, "rtc.nem")
        image[offset+768:offset+1024]= create_dir_entry(inode_acpi, 1, "acpi.nem")
        image[offset+1024:offset+1280]= create_dir_entry(inode_pci, 1, "pci.nem")
        image[offset+1280:offset+1536]= create_dir_entry(inode_ata, 1, "ata.nem")
        image[offset+1536:offset+1792]= create_dir_entry(inode_ahci, 1, "ahci.nem")
        image[offset+1792:offset+2048]= create_dir_entry(inode_e1000, 1, "e1000.nem")
        image[offset+2048:offset+2304]= create_dir_entry(inode_ps2mouse, 1, "ps2mouse.nem")
        image[offset+2304:offset+2560]= create_dir_entry(inode_virtio_blk, 1, "virtio-blk.nem")

        # NEM driver data blocks
        for inum, (fname, fdata) in nem_data.items():
            if not fdata:
                continue
            blks = nem_blocks.get(inum, [])
            print(f"[*] Writing System\\Drivers\\{fname} content ({len(fdata)} bytes)...")
            for bi, blk in enumerate(blks):
                chunk = fdata[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (DATA_START_SECTOR + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # System\Libraries\ directory
        print("[*] Writing System\\Libraries directory...")
        blk = lib_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256]     = create_dir_entry(inode_fsnxl, 1, "fs.nxl")
        image[offset+512:offset+768]  = create_dir_entry(inode_consolenxl, 1, "console.nxl")
        image[offset+1024:offset+1280]= create_dir_entry(inode_mathnxl, 1, "math.nxl")

        # fs.nxl (libneodos) data blocks
        if nxl_data:
            for (inum, blklist) in [(inode_fsnxl, fs_nxl_blocks)]:
                print(f"[*] Writing System\\Libraries inode {inum} ({len(nxl_data)} bytes)...")
                for bi, blk in enumerate(blklist):
                    chunk = nxl_data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                    off = (DATA_START_SECTOR + blk * 8) * 512
                    image[off:off+len(chunk)] = chunk

        # math.nxl data blocks
        if math_nxl_data:
            print(f"[*] Writing System\\Libraries\\math.nxl ({len(math_nxl_data)} bytes)...")
            for bi, blk in enumerate(math_nxl_blocks):
                chunk = math_nxl_data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (DATA_START_SECTOR + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # console.nxl data blocks
        if console_nxl_data:
            print(f"[*] Writing System\\Libraries\\console.nxl ({len(console_nxl_data)} bytes)...")
            for bi, blk in enumerate(console_nxl_blocks):
                chunk = console_nxl_data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (DATA_START_SECTOR + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # System\Layouts\ directory
        print("[*] Writing System\\Layouts directory...")
        blk = lay_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256]  = create_dir_entry(inode_eses, 1, "es-ES.nkb")
        image[offset+256:offset+512]= create_dir_entry(inode_enus, 1, "en-US.nkb")

        # NKB data blocks
        for inum, content, blk_list in [(inode_eses, es_nkb_content, es_nkb_blocks), (inode_enus, en_nkb_content, en_nkb_blocks)]:
            for bi, blk in enumerate(blk_list):
                chunk = content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (DATA_START_SECTOR + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # System\Config\ directory
        print("[*] Writing System\\Config directory...")
        blk = cfg_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256]  = create_dir_entry(inode_systemcfg, 1, "system.cfg")
        image[offset+256:offset+512]= create_dir_entry(inode_inputcfg, 1, "input.cfg")

        # system.cfg content
        print("[*] Writing system.cfg...")
        for bi, blk in enumerate(system_cfg_blocks):
            chunk = system_cfg_content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
            off = (DATA_START_SECTOR + blk * 8) * 512
            image[off:off+len(chunk)] = chunk

        # input.cfg content
        print("[*] Writing input.cfg...")
        for bi, blk in enumerate(input_cfg_blocks):
            chunk = input_cfg_content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
            off = (DATA_START_SECTOR + blk * 8) * 512
            image[off:off+len(chunk)] = chunk

        # ── Programs\ directory ──
        print("[*] Writing Programs directory...")
        blk = prog_dir_blocks[0]
        offset = (DATA_START_SECTOR + blk * 8) * 512
        image[offset:offset+256]      = create_dir_entry(inode_neoshell, 1, "NeoShell.nxe")
        image[offset+256:offset+512]  = create_dir_entry(inode_neoinit, 1, "NeoInit.nxe")
        image[offset+512:offset+768]  = create_dir_entry(inode_cpuinfo, 1, "cpuinfo.nxe")
        image[offset+768:offset+1024] = create_dir_entry(inode_coredir, 1, "dir.nxe")
        image[offset+1024:offset+1280]= create_dir_entry(inode_corehelp, 1, "help.nxe")
        image[offset+1280:offset+1536]= create_dir_entry(inode_datetime, 1, "datetime.nxe")
        image[offset+1536:offset+1792]= create_dir_entry(inode_ver, 1, "ver.nxe")
        image[offset+1792:offset+2048]= create_dir_entry(inode_neomem, 1, "neomem.nxe")
        image[offset+2048:offset+2304]= create_dir_entry(inode_vol, 1, "vol.nxe")
        image[offset+2304:offset+2560]= create_dir_entry(inode_echo, 1, "echo.nxe")
        image[offset+2560:offset+2816]= create_dir_entry(inode_cd, 1, "cd.nxe")
        image[offset+2816:offset+3072]= create_dir_entry(inode_coretype, 1, "type.nxe")
        image[offset+3328:offset+3584]= create_dir_entry(inode_tree, 1, "tree.nxe")
        image[offset+3584:offset+3840]= create_dir_entry(inode_corecls, 1, "cls.nxe")
        image[offset+3840:offset+4096]= create_dir_entry(inode_corecopy, 1, "copy.nxe")
        image[offset+4096:offset+4352]= create_dir_entry(inode_coredel, 1, "del.nxe")
        image[offset+4352:offset+4608]= create_dir_entry(inode_coreren, 1, "ren.nxe")
        image[offset+4608:offset+4864]= create_dir_entry(inode_coremd, 1, "md.nxe")
        image[offset+4864:offset+5120]= create_dir_entry(inode_corerd, 1, "rd.nxe")
        image[offset+5120:offset+5376]= create_dir_entry(inode_cmdtest, 1, "cmdtest.nxe")
        image[offset+5376:offset+5632]= create_dir_entry(inode_drives_nxe, 1, "drives.nxe")
        image[offset+5632:offset+5888]= create_dir_entry(inode_ps, 1, "ps.nxe")
        image[offset+5888:offset+6144]= create_dir_entry(inode_keyb, 1, "keyb.nxe")
        image[offset+6144:offset+6400]= create_dir_entry(inode_kill, 1, "kill.nxe")
        image[offset+6400:offset+6656]= create_dir_entry(inode_pri, 1, "pri.nxe")
        image[offset+6656:offset+6912]= create_dir_entry(inode_label, 1, "label.nxe")
        image[offset+6912:offset+7168]= create_dir_entry(inode_fsck, 1, "fsck.nxe")
        image[offset+7168:offset+7424]= create_dir_entry(inode_ndreg, 1, "ndreg.nxe")
        image[offset+7424:offset+7680]= create_dir_entry(inode_loadnem, 1, "loadnem.nxe")
        image[offset+7680:offset+7936]= create_dir_entry(inode_progress, 1, "progress.nxe")
        image[offset+7936:offset+8192]= create_dir_entry(inode_neotop, 1, "neotop.nxe")

        # Write all NXE binary data
        nxe_inode_map = {
            inode_neoshell: ('NeoShell.nxe', nxe_files['neoshell']),
            inode_neoinit: ('NeoInit.nxe', nxe_files['neoinit']),
            inode_cpuinfo: ('cpuinfo.nxe', nxe_files['cpuinfo']),
            inode_coredir: ('dir.nxe', nxe_files['coredir']),
            inode_corehelp: ('help.nxe', nxe_files['corehelp']),
            inode_datetime: ('datetime.nxe', nxe_files['datetime']),
            inode_ver: ('ver.nxe', nxe_files['ver']),
            inode_neomem: ('neomem.nxe', nxe_files['neomem']),
            inode_vol: ('vol.nxe', nxe_files['vol']),
            inode_echo: ('echo.nxe', nxe_files['echo']),
            inode_cd: ('cd.nxe', nxe_files['cd']),
            inode_coretype: ('type.nxe', nxe_files['coretype']),
            inode_tree: ('tree.nxe', nxe_files['tree']),
            inode_corecls: ('cls.nxe', nxe_files['corecls']),
            inode_corecopy: ('copy.nxe', nxe_files['corecopy']),
            inode_coredel: ('del.nxe', nxe_files['coredel']),
            inode_coreren: ('ren.nxe', nxe_files['coreren']),
            inode_coremd: ('md.nxe', nxe_files['coremd']),
            inode_corerd: ('rd.nxe', nxe_files['corerd']),
            inode_cmdtest: ('cmdtest.nxe', nxe_files['cmdtest']),
            inode_drives_nxe: ('drives.nxe', nxe_files['drives']),
            inode_ps: ('ps.nxe', nxe_files['ps']),
            inode_keyb: ('keyb.nxe', nxe_files['keyb']),
            inode_kill: ('kill.nxe', nxe_files['kill']),
            inode_pri: ('pri.nxe', nxe_files['pri']),
            inode_label: ('label.nxe', nxe_files['label']),
            inode_fsck: ('fsck.nxe', nxe_files['fsck']),
            inode_ndreg: ('ndreg.nxe', nxe_files['ndreg']),
            inode_loadnem: ('loadnem.nxe', nxe_files['loadnem']),
            inode_progress: ('progress.nxe', nxe_files['progress']),
            inode_neotop: ('neotop.nxe', nxe_files['neotop']),
        }
        for inum, (name, data) in nxe_inode_map.items():
            if not data:
                continue
            blks = block_allocs.get(inum, [])
            print(f"[*] Writing Programs\\{name} ({len(data)} bytes across {len(blks)} blocks)...")
            for bi, blk in enumerate(blks):
                chunk = data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (DATA_START_SECTOR + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        allocator.check_no_duplicates()

        # Escribir imagen a disco
    output_file = args.output
    print(f"[*] Writing image to {output_file}...")
    with open(output_file, 'wb') as f:
        f.write(image)
    
    print(f"[+] Image created: {output_file} ({len(image)} bytes)")

if __name__ == '__main__':
    main()
