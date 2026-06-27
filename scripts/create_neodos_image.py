#!/usr/bin/env python3
import struct
import sys
import os

BLOCK_SIZE = 4096
SECTOR_SIZE = 512
SUPERBLOCK_MAGIC = 0x4F444F4E  # "NEOD"
DATA_START_SECTOR = 200
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
        dir_mode = MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D
        root_inode = create_inode(0, dir_mode, 256, [ROOT_DIR_BLOCK, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512:512+256] = root_inode
        txt_inode = create_inode(1, MODE_FILE | default_perms_for_filename("test.txt"), 56, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+256:512+512] = txt_inode

        # Root dir
        offset = (DATA_START_SECTOR + ROOT_DIR_BLOCK * 8) * 512
        entry = create_dir_entry(1, 1, "test.txt")
        image[offset:offset+256] = entry

        # Data block 1
        offset = (200 + 8) * 512
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

        # Inodes 0-2: root dir, readme.txt, test.bat
        root_inode = create_inode(0, dir_mode, BLOCK_SIZE, [ROOT_DIR_BLOCK, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512:512+256] = root_inode
        readme_inode = create_inode(1, MODE_FILE | default_perms_for_filename("readme.txt"), 1024, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+256:512+512] = readme_inode
        testbat_inode = create_inode(2, MODE_FILE | default_perms_for_filename("test.bat"), 512, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        image[512+512:512+768] = testbat_inode

        # ── Read binary data ──
        userbin_dir = os.path.join(os.path.dirname(__file__), '..', 'userbin')
        nxe_files = {}
        for name in ['cpuinfo', 'neoshell', 'neoinit', 'coredir', 'cd', 'corehelp', 'datetime', 'ver', 'neomem', 'vol', 'echo', 'label', 'kobj', 'coretype', 'tree', 'corecls', 'corecopy', 'coredel', 'coreren', 'coremd', 'corerd', 'cmdtest', 'drives', 'ps', 'keyb', 'kill', 'pri', 'fsck', 'ndreg', 'loadnem', 'progress', 'neotop']:
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

        # ── Inode allocation map ──
        # 0=Root, 1=readme.txt, 2=test.bat
        # 3=System, 4=Kernel, 5=boot.cfg
        # 6=Drivers, 7-13=NEM drivers
        # 14=Libraries, 15-18,44,64=NXL files
        # 19=Layouts, 20-21=NKB files
        # 22=Config, 23-24=cfg files
        # 25=Programs, 26-36,45-53,65,67=NXE files
        # 37=Packages, 38=Users, 39=Default, 40=Alejandro
        # 41=Temp, 42=Data, 43=Logs, 46=type.nxe, 47=tree.nxe
        # 48=cls.nxe, 49=copy.nxe, 50=del.nxe, 51=ren.nxe, 52=md.nxe, 53=rd.nxe

        # Read NEM driver data
        nem_data = {
            7:  ("ps2kbd.nem", read_nem("BOOT", "ps2kbd.nem")),
            8:  ("serial.nem",   read_nem("BOOT", "serial.nem")),
            9:  ("rtc.nem",      read_nem("BOOT", "rtc.nem")),
            10: ("acpi.nem",     read_nem("SYSTEM", "acpi.nem")),
            11: ("pci.nem",      read_nem("SYSTEM", "pci.nem")),
            12: ("ata.nem",     read_nem("SYSTEM", "ata.nem")),
            13: ("ahci.nem",     read_nem("SYSTEM", "ahci.nem")),
            16: ("ps2mouse.nem", read_nem("BOOT", "ps2mouse.nem")),
        }

        # Allocate blocks for everything
        cpuinfo_blocks    = alloc_blocks(28, len(nxe_files['cpuinfo']))
        neoshell_blocks   = alloc_blocks(26, len(nxe_files['neoshell']))
        neoinit_blocks    = alloc_blocks(27, len(nxe_files['neoinit']))
        coredir_blocks    = alloc_blocks(29, len(nxe_files['coredir']))
        cd_blocks         = alloc_blocks(36, len(nxe_files['cd']))
        corehelp_blocks   = alloc_blocks(30, len(nxe_files['corehelp']))
        datetime_blocks   = alloc_blocks(31, len(nxe_files['datetime']))
        ver_blocks        = alloc_blocks(32, len(nxe_files['ver']))
        neomem_blocks     = alloc_blocks(33, len(nxe_files['neomem']))
        vol_blocks        = alloc_blocks(34, len(nxe_files['vol']))
        echo_blocks       = alloc_blocks(35, len(nxe_files['echo']))
        kobj_blocks       = alloc_blocks(45, len(nxe_files['kobj']))
        coretype_blocks   = alloc_blocks(46, len(nxe_files['coretype']))
        tree_blocks       = alloc_blocks(47, len(nxe_files['tree']))
        corecls_blocks    = alloc_blocks(48, len(nxe_files['corecls']))
        corecopy_blocks   = alloc_blocks(49, len(nxe_files['corecopy']))
        coredel_blocks    = alloc_blocks(50, len(nxe_files['coredel']))
        coreren_blocks    = alloc_blocks(51, len(nxe_files['coreren']))
        coremd_blocks     = alloc_blocks(52, len(nxe_files['coremd']))
        corerd_blocks     = alloc_blocks(53, len(nxe_files['corerd']))
        cmdtest_blocks    = alloc_blocks(54, len(nxe_files['cmdtest']))
        drives_blocks     = alloc_blocks(55, len(nxe_files['drives']))
        ps_blocks         = alloc_blocks(56, len(nxe_files['ps']))
        keyb_blocks       = alloc_blocks(57, len(nxe_files['keyb']))
        kill_blocks       = alloc_blocks(58, len(nxe_files['kill']))
        pri_blocks        = alloc_blocks(59, len(nxe_files['pri']))
        label_blocks      = alloc_blocks(60, len(nxe_files['label']))
        fsck_blocks       = alloc_blocks(61, len(nxe_files['fsck']))
        ndreg_blocks      = alloc_blocks(62, len(nxe_files['ndreg']))
        loadnem_blocks    = alloc_blocks(63, len(nxe_files['loadnem']))
        progress_blocks   = alloc_blocks(65, len(nxe_files['progress']))
        neotop_blocks     = alloc_blocks(67, len(nxe_files['neotop']))
        fs_nxl_blocks     = alloc_blocks(15, len(nxl_data))
        math_nxl_blocks   = alloc_blocks(44, len(math_nxl_data))
        console_nxl_blocks = alloc_blocks(64, len(console_nxl_data))
        bootcfg_blocks    = alloc_blocks(5, len(bootcfg_content))
        system_cfg_blocks = alloc_blocks(23, len(system_cfg_content))
        input_cfg_blocks  = alloc_blocks(24, len(input_cfg_content))
        es_nkb_blocks     = alloc_blocks(20, len(es_nkb_content))
        en_nkb_blocks     = alloc_blocks(21, len(en_nkb_content))

        nem_blocks = {}
        for inum in nem_data:
            nem_blocks[inum] = alloc_blocks(inum, len(nem_data[inum][1]))

        # Directory blocks (fixed block allocation)
        sys_dir_blocks    = alloc_blocks(3, 2304)    # System dir (9 entries × 256)
        kernel_dir_blocks = alloc_blocks(4, 512)     # Kernel dir
        drv_dir_blocks    = alloc_blocks(6, 2048)    # Drivers dir (8 entries)
        lib_dir_blocks    = alloc_blocks(14, 1536)   # Libraries dir (5 entries)
        lay_dir_blocks    = alloc_blocks(19, 768)    # Layouts dir (2 entries + padding)
        cfg_dir_blocks    = alloc_blocks(22, 768)    # Config dir (2 entries + padding)
        prog_dir_blocks   = alloc_blocks(25, 8192)   # Programs dir (32 entries)
        pkg_dir_blocks    = alloc_blocks(37, 256)    # Packages dir (empty)
        usr_dir_blocks    = alloc_blocks(38, 768)    # Users dir
        def_dir_blocks    = alloc_blocks(39, 256)    # Users\Default (empty)
        ale_dir_blocks    = alloc_blocks(40, 256)    # Users\Alejandro (empty)
        tmp_dir_blocks    = alloc_blocks(41, 256)    # Temp dir (empty)
        dat_dir_blocks    = alloc_blocks(42, 256)    # Data dir (empty)
        log_dir_blocks    = alloc_blocks(43, 256)    # Logs dir (empty)

        # ── Write inode table ──
        inodes_data = {
            3:  (dir_mode, 2304, pad_blocks(sys_dir_blocks)),
            4:  (dir_mode, 512,  pad_blocks(kernel_dir_blocks)),
            5:  (MODE_FILE | default_perms_for_filename("boot.cfg"), len(bootcfg_content), pad_blocks(bootcfg_blocks)),
            6:  (dir_mode, 2048, pad_blocks(drv_dir_blocks)),
            14: (dir_mode, 1536, pad_blocks(lib_dir_blocks)),
            15: (MODE_FILE | default_perms_for_filename("fs.nxl"), len(nxl_data), pad_blocks(fs_nxl_blocks)),
            44: (MODE_FILE | default_perms_for_filename("math.nxl"), len(math_nxl_data), pad_blocks(math_nxl_blocks)),
            64: (MODE_FILE | default_perms_for_filename("console.nxl"), len(console_nxl_data), pad_blocks(console_nxl_blocks)),
            19: (dir_mode, 768,  pad_blocks(lay_dir_blocks)),
            20: (MODE_FILE | default_perms_for_filename("es-ES.nkb"), len(es_nkb_content), pad_blocks(es_nkb_blocks)),
            21: (MODE_FILE | default_perms_for_filename("en-US.nkb"), len(en_nkb_content), pad_blocks(en_nkb_blocks)),
            22: (dir_mode, 768,  pad_blocks(cfg_dir_blocks)),
            23: (MODE_FILE | default_perms_for_filename("system.cfg"), len(system_cfg_content), pad_blocks(system_cfg_blocks)),
            24: (MODE_FILE | default_perms_for_filename("input.cfg"), len(input_cfg_content), pad_blocks(input_cfg_blocks)),
            25: (dir_mode, 6656, pad_blocks(prog_dir_blocks)),
            26: (MODE_FILE | default_perms_for_filename("NeoShell.nxe"), len(nxe_files['neoshell']), pad_blocks(neoshell_blocks)),
            27: (MODE_FILE | default_perms_for_filename("NeoInit.nxe"), len(nxe_files['neoinit']), pad_blocks(neoinit_blocks)),
            28: (MODE_FILE | default_perms_for_filename("cpuinfo.nxe"), len(nxe_files['cpuinfo']), pad_blocks(cpuinfo_blocks)),
            29: (MODE_FILE | default_perms_for_filename("dir.nxe"), len(nxe_files['coredir']), pad_blocks(coredir_blocks)),
            36: (MODE_FILE | default_perms_for_filename("cd.nxe"), len(nxe_files['cd']), pad_blocks(cd_blocks)),
            30: (MODE_FILE | default_perms_for_filename("help.nxe"), len(nxe_files['corehelp']), pad_blocks(corehelp_blocks)),
            31: (MODE_FILE | default_perms_for_filename("datetime.nxe"), len(nxe_files['datetime']), pad_blocks(datetime_blocks)),
            32: (MODE_FILE | default_perms_for_filename("ver.nxe"), len(nxe_files['ver']), pad_blocks(ver_blocks)),
            33: (MODE_FILE | default_perms_for_filename("neomem.nxe"), len(nxe_files['neomem']), pad_blocks(neomem_blocks)),
            34: (MODE_FILE | default_perms_for_filename("vol.nxe"), len(nxe_files['vol']), pad_blocks(vol_blocks)),
            35: (MODE_FILE | default_perms_for_filename("echo.nxe"), len(nxe_files['echo']), pad_blocks(echo_blocks)),
             45: (MODE_FILE | default_perms_for_filename("kobj.nxe"), len(nxe_files['kobj']), pad_blocks(kobj_blocks)),
              46: (MODE_FILE | default_perms_for_filename("type.nxe"), len(nxe_files['coretype']), pad_blocks(coretype_blocks)),
              47: (MODE_FILE | default_perms_for_filename("tree.nxe"), len(nxe_files['tree']), pad_blocks(tree_blocks)),
              48: (MODE_FILE | default_perms_for_filename("cls.nxe"), len(nxe_files['corecls']), pad_blocks(corecls_blocks)),
              49: (MODE_FILE | default_perms_for_filename("copy.nxe"), len(nxe_files['corecopy']), pad_blocks(corecopy_blocks)),
              50: (MODE_FILE | default_perms_for_filename("del.nxe"), len(nxe_files['coredel']), pad_blocks(coredel_blocks)),
              51: (MODE_FILE | default_perms_for_filename("ren.nxe"), len(nxe_files['coreren']), pad_blocks(coreren_blocks)),
              52: (MODE_FILE | default_perms_for_filename("md.nxe"), len(nxe_files['coremd']), pad_blocks(coremd_blocks)),
              53: (MODE_FILE | default_perms_for_filename("rd.nxe"), len(nxe_files['corerd']), pad_blocks(corerd_blocks)),
              54: (MODE_FILE | default_perms_for_filename("cmdtest.nxe"), len(nxe_files['cmdtest']), pad_blocks(cmdtest_blocks)),
               55: (MODE_FILE | default_perms_for_filename("drives.nxe"), len(nxe_files['drives']), pad_blocks(drives_blocks)),
              37: (dir_mode, 256,  pad_blocks(pkg_dir_blocks)),
             38: (dir_mode, 768,  pad_blocks(usr_dir_blocks)),
             39: (dir_mode, 256,  pad_blocks(def_dir_blocks)),
             40: (dir_mode, 256,  pad_blocks(ale_dir_blocks)),
             56: (MODE_FILE | default_perms_for_filename("ps.nxe"), len(nxe_files['ps']), pad_blocks(ps_blocks)),
             57: (MODE_FILE | default_perms_for_filename("keyb.nxe"), len(nxe_files['keyb']), pad_blocks(keyb_blocks)),
             58: (MODE_FILE | default_perms_for_filename("kill.nxe"), len(nxe_files['kill']), pad_blocks(kill_blocks)),
              59: (MODE_FILE | default_perms_for_filename("pri.nxe"), len(nxe_files['pri']), pad_blocks(pri_blocks)),
               60: (MODE_FILE | default_perms_for_filename("label.nxe"), len(nxe_files['label']), pad_blocks(label_blocks)),
              61: (MODE_FILE | default_perms_for_filename("fsck.nxe"), len(nxe_files['fsck']), pad_blocks(fsck_blocks)),
              62: (MODE_FILE | default_perms_for_filename("ndreg.nxe"), len(nxe_files['ndreg']), pad_blocks(ndreg_blocks)),
              63: (MODE_FILE | default_perms_for_filename("loadnem.nxe"), len(nxe_files['loadnem']), pad_blocks(loadnem_blocks)),
              65: (MODE_FILE | default_perms_for_filename("progress.nxe"), len(nxe_files['progress']), pad_blocks(progress_blocks)),
              67: (MODE_FILE | default_perms_for_filename("neotop.nxe"), len(nxe_files['neotop']), pad_blocks(neotop_blocks)),
            41: (dir_mode, 256,  pad_blocks(tmp_dir_blocks)),
            42: (dir_mode, 256,  pad_blocks(dat_dir_blocks)),
            43: (dir_mode, 256,  pad_blocks(log_dir_blocks)),
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
        image[offset:offset+256]   = create_dir_entry(1, 1, "readme.txt")
        image[offset+256:offset+512] = create_dir_entry(2, 1, "test.bat")
        image[offset+512:offset+768] = create_dir_entry(3, 2, "System")
        image[offset+768:offset+1024] = create_dir_entry(25, 2, "Programs")
        image[offset+1024:offset+1280] = create_dir_entry(37, 2, "Packages")
        image[offset+1280:offset+1536] = create_dir_entry(38, 2, "Users")
        image[offset+1536:offset+1792] = create_dir_entry(41, 2, "Temp")
        image[offset+1792:offset+2048] = create_dir_entry(42, 2, "Data")
        image[offset+2048:offset+2304] = create_dir_entry(43, 2, "Logs")

        # ── Data blocks ──
        # Block 1 = readme.txt
        print("[*] Writing readme.txt content...")
        offset = (200 + 8) * 512
        image[offset:offset+len(readme_text)] = readme_text.encode('utf-8')

        # Block 2 = test.bat
        print("[*] Writing test.bat content...")
        offset = (200 + 16) * 512
        testbat_content = read_cfg("test.bat")
        image[offset:offset+len(testbat_content)] = testbat_content

        # ── System\ directory ──
        print("[*] Writing System directory...")
        blk = sys_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256]      = create_dir_entry(4, 2, "Kernel")
        image[offset+256:offset+512]  = create_dir_entry(6, 2, "Drivers")
        image[offset+512:offset+768]  = create_dir_entry(14, 2, "Libraries")
        image[offset+768:offset+1024] = create_dir_entry(19, 2, "Layouts")
        image[offset+1024:offset+1280]= create_dir_entry(22, 2, "Config")

        # System\Kernel\
        print("[*] Writing System\\Kernel directory...")
        blk = kernel_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256] = create_dir_entry(5, 1, "boot.cfg")

        # System\Kernel\boot.cfg content
        print("[*] Writing boot.cfg...")
        for bi, blk in enumerate(bootcfg_blocks):
            chunk = bootcfg_content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
            off = (200 + blk * 8) * 512
            image[off:off+len(chunk)] = chunk

        # System\Drivers\ directory
        print("[*] Writing System\\Drivers directory...")
        blk = drv_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256]    = create_dir_entry(7, 1, "kbps2.nem")
        image[offset+256:offset+512]= create_dir_entry(8, 1, "serial.nem")
        image[offset+512:offset+768]= create_dir_entry(9, 1, "rtc.nem")
        image[offset+768:offset+1024]= create_dir_entry(10, 1, "acpi.nem")
        image[offset+1024:offset+1280]= create_dir_entry(11, 1, "pci.nem")
        image[offset+1280:offset+1536]= create_dir_entry(12, 1, "ata.nem")
        image[offset+1536:offset+1792]= create_dir_entry(13, 1, "ahci.nem")
        image[offset+1792:offset+2048]= create_dir_entry(16, 1, "ps2mouse.nem")

        # NEM driver data blocks
        for inum, (fname, fdata) in nem_data.items():
            if not fdata:
                continue
            blks = nem_blocks.get(inum, [])
            print(f"[*] Writing System\\Drivers\\{fname} content ({len(fdata)} bytes)...")
            for bi, blk in enumerate(blks):
                chunk = fdata[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (200 + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # System\Libraries\ directory
        print("[*] Writing System\\Libraries directory...")
        blk = lib_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256]     = create_dir_entry(15, 1, "fs.nxl")
        image[offset+512:offset+768]  = create_dir_entry(64, 1, "console.nxl")
        image[offset+1024:offset+1280]= create_dir_entry(44, 1, "math.nxl")

        # fs.nxl (libneodos) data blocks
        if nxl_data:
            for (inum, blklist) in [(15, fs_nxl_blocks)]:
                print(f"[*] Writing System\\Libraries inode {inum} ({len(nxl_data)} bytes)...")
                for bi, blk in enumerate(blklist):
                    chunk = nxl_data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                    off = (200 + blk * 8) * 512
                    image[off:off+len(chunk)] = chunk

        # math.nxl data blocks
        if math_nxl_data:
            print(f"[*] Writing System\\Libraries\\math.nxl ({len(math_nxl_data)} bytes)...")
            for bi, blk in enumerate(math_nxl_blocks):
                chunk = math_nxl_data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (200 + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # console.nxl data blocks
        if console_nxl_data:
            print(f"[*] Writing System\\Libraries\\console.nxl ({len(console_nxl_data)} bytes)...")
            for bi, blk in enumerate(console_nxl_blocks):
                chunk = console_nxl_data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (200 + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # System\Layouts\ directory
        print("[*] Writing System\\Layouts directory...")
        blk = lay_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256]  = create_dir_entry(20, 1, "es-ES.nkb")
        image[offset+256:offset+512]= create_dir_entry(21, 1, "en-US.nkb")

        # NKB data blocks
        for inum, content, blk_list in [(20, es_nkb_content, es_nkb_blocks), (21, en_nkb_content, en_nkb_blocks)]:
            for bi, blk in enumerate(blk_list):
                chunk = content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (200 + blk * 8) * 512
                image[off:off+len(chunk)] = chunk

        # System\Config\ directory
        print("[*] Writing System\\Config directory...")
        blk = cfg_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256]  = create_dir_entry(23, 1, "system.cfg")
        image[offset+256:offset+512]= create_dir_entry(24, 1, "input.cfg")

        # system.cfg content
        print("[*] Writing system.cfg...")
        for bi, blk in enumerate(system_cfg_blocks):
            chunk = system_cfg_content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
            off = (200 + blk * 8) * 512
            image[off:off+len(chunk)] = chunk

        # input.cfg content
        print("[*] Writing input.cfg...")
        for bi, blk in enumerate(input_cfg_blocks):
            chunk = input_cfg_content[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
            off = (200 + blk * 8) * 512
            image[off:off+len(chunk)] = chunk

        # ── Programs\ directory ──
        print("[*] Writing Programs directory...")
        blk = prog_dir_blocks[0]
        offset = (200 + blk * 8) * 512
        image[offset:offset+256]      = create_dir_entry(26, 1, "NeoShell.nxe")
        image[offset+256:offset+512]  = create_dir_entry(27, 1, "NeoInit.nxe")
        image[offset+512:offset+768]  = create_dir_entry(28, 1, "cpuinfo.nxe")
        image[offset+768:offset+1024] = create_dir_entry(29, 1, "dir.nxe")
        image[offset+1024:offset+1280]= create_dir_entry(30, 1, "help.nxe")
        image[offset+1280:offset+1536]= create_dir_entry(31, 1, "datetime.nxe")
        image[offset+1536:offset+1792]= create_dir_entry(32, 1, "ver.nxe")
        image[offset+1792:offset+2048]= create_dir_entry(33, 1, "neomem.nxe")
        image[offset+2048:offset+2304]= create_dir_entry(34, 1, "vol.nxe")
        image[offset+2304:offset+2560]= create_dir_entry(35, 1, "echo.nxe")
        image[offset+2560:offset+2816]= create_dir_entry(36, 1, "cd.nxe")
        image[offset+2816:offset+3072]= create_dir_entry(45, 1, "kobj.nxe")
        image[offset+3072:offset+3328]= create_dir_entry(46, 1, "type.nxe")
        image[offset+3328:offset+3584]= create_dir_entry(47, 1, "tree.nxe")
        image[offset+3584:offset+3840]= create_dir_entry(48, 1, "cls.nxe")
        image[offset+3840:offset+4096]= create_dir_entry(49, 1, "copy.nxe")
        image[offset+4096:offset+4352]= create_dir_entry(50, 1, "del.nxe")
        image[offset+4352:offset+4608]= create_dir_entry(51, 1, "ren.nxe")
        image[offset+4608:offset+4864]= create_dir_entry(52, 1, "md.nxe")
        image[offset+4864:offset+5120]= create_dir_entry(53, 1, "rd.nxe")
        image[offset+5120:offset+5376]= create_dir_entry(54, 1, "cmdtest.nxe")
        image[offset+5376:offset+5632]= create_dir_entry(55, 1, "drives.nxe")
        image[offset+5632:offset+5888]= create_dir_entry(56, 1, "ps.nxe")
        image[offset+5888:offset+6144]= create_dir_entry(57, 1, "keyb.nxe")
        image[offset+6144:offset+6400]= create_dir_entry(58, 1, "kill.nxe")
        image[offset+6400:offset+6656]= create_dir_entry(59, 1, "pri.nxe")
        image[offset+6656:offset+6912]= create_dir_entry(60, 1, "label.nxe")
        image[offset+6912:offset+7168]= create_dir_entry(61, 1, "fsck.nxe")
        image[offset+7168:offset+7424]= create_dir_entry(62, 1, "ndreg.nxe")
        image[offset+7424:offset+7680]= create_dir_entry(63, 1, "loadnem.nxe")
        image[offset+7680:offset+7936]= create_dir_entry(65, 1, "progress.nxe")
        image[offset+7936:offset+8192]= create_dir_entry(67, 1, "neotop.nxe")

        # Write all NXE binary data
        nxe_inode_map = {
            26: ('NeoShell.nxe', nxe_files['neoshell']),
            27: ('NeoInit.nxe', nxe_files['neoinit']),
            28: ('cpuinfo.nxe', nxe_files['cpuinfo']),
            29: ('dir.nxe', nxe_files['coredir']),
            30: ('help.nxe', nxe_files['corehelp']),
            31: ('datetime.nxe', nxe_files['datetime']),
            32: ('ver.nxe', nxe_files['ver']),
            33: ('neomem.nxe', nxe_files['neomem']),
            34: ('vol.nxe', nxe_files['vol']),
            35: ('echo.nxe', nxe_files['echo']),
            36: ('cd.nxe', nxe_files['cd']),
            45: ('kobj.nxe', nxe_files['kobj']),
             46: ('type.nxe', nxe_files['coretype']),
             47: ('tree.nxe', nxe_files['tree']),
             48: ('cls.nxe', nxe_files['corecls']),
             49: ('copy.nxe', nxe_files['corecopy']),
             50: ('del.nxe', nxe_files['coredel']),
             51: ('ren.nxe', nxe_files['coreren']),
             52: ('md.nxe', nxe_files['coremd']),
              53: ('rd.nxe', nxe_files['corerd']),
              54: ('cmdtest.nxe', nxe_files['cmdtest']),
               55: ('drives.nxe', nxe_files['drives']),
              56: ('ps.nxe', nxe_files['ps']),
              57: ('keyb.nxe', nxe_files['keyb']),
              58: ('kill.nxe', nxe_files['kill']),
               59: ('pri.nxe', nxe_files['pri']),
               60: ('label.nxe', nxe_files['label']),
            61: ('fsck.nxe', nxe_files['fsck']),
            62: ('ndreg.nxe', nxe_files['ndreg']),
            63: ('loadnem.nxe', nxe_files['loadnem']),
            65: ('progress.nxe', nxe_files['progress']),
            67: ('neotop.nxe', nxe_files['neotop']),
        }
        for inum, (name, data) in nxe_inode_map.items():
            if not data:
                continue
            blks = block_allocs.get(inum, [])
            print(f"[*] Writing Programs\\{name} ({len(data)} bytes across {len(blks)} blocks)...")
            for bi, blk in enumerate(blks):
                chunk = data[bi * BLOCK_SIZE:(bi + 1) * BLOCK_SIZE]
                off = (200 + blk * 8) * 512
                image[off:off+len(chunk)] = chunk
    
    # Escribir imagen a disco
    output_file = args.output
    print(f"[*] Writing image to {output_file}...")
    with open(output_file, 'wb') as f:
        f.write(image)
    
    print(f"[+] Image created: {output_file} ({len(image)} bytes)")

if __name__ == '__main__':
    main()
