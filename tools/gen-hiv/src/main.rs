use std::collections::BTreeMap;
use std::path::PathBuf;
use std::{env, fs, process};

const NEOH_MAGIC: u32 = 0x484F454E;
const NEOH_VERSION: u32 = 1;
const NULL_CELL: u32 = 0xFFFFFFFF;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 2;

fn put_u32_le(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

fn put_u16_le(buf: &mut [u8], off: usize, val: u16) {
    buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
}

struct KeyMeta {
    name: String,
    parent: u32,
}

struct ValueMeta {
    name: String,
    type_name: &'static str,
    display: String,
}

struct HiveBuilder {
    cells: BTreeMap<u32, Vec<u8>>,
    next_idx: u32,
    keys: BTreeMap<u32, KeyMeta>,
    values: BTreeMap<u32, ValueMeta>,
    // Track key → [value_id] for printing
    key_values: BTreeMap<u32, Vec<u32>>,
}

impl HiveBuilder {
    fn new() -> Self {
        Self {
            cells: BTreeMap::new(),
            next_idx: 1,
            keys: BTreeMap::new(),
            values: BTreeMap::new(),
            key_values: BTreeMap::new(),
        }
    }

    fn add_key(&mut self, idx: u32, name: &str, parent: u32,
               subkeys_head: u32, subkeys_sibling: u32,
               values_head: u32, sec_desc: u32, last_write: u64) {
        let name_bytes = name.as_bytes();
        let mut buf = vec![0u8; 5 + 2 + name_bytes.len() + 4 * 5 + 8];
        put_u32_le(&mut buf, 0, idx);
        buf[4] = 1;
        put_u16_le(&mut buf, 5, name_bytes.len() as u16);
        buf[7..7 + name_bytes.len()].copy_from_slice(name_bytes);
        let mut pos = 7 + name_bytes.len();
        put_u32_le(&mut buf, pos, parent); pos += 4;
        put_u32_le(&mut buf, pos, subkeys_head); pos += 4;
        put_u32_le(&mut buf, pos, subkeys_sibling); pos += 4;
        put_u32_le(&mut buf, pos, values_head); pos += 4;
        put_u32_le(&mut buf, pos, sec_desc); pos += 4;
        put_u32_le(&mut buf, pos, last_write as u32); pos += 4;
        put_u32_le(&mut buf, pos, (last_write >> 32) as u32);
        self.cells.insert(idx, buf);
        self.keys.insert(idx, KeyMeta { name: name.to_string(), parent });
    }

    fn add_value(&mut self, idx: u32, name: &str, value_type: u32,
                 data: &[u8], next_val: u32, parent_key: u32) {
        let name_bytes = name.as_bytes();
        let mut buf = vec![0u8; 5 + 2 + name_bytes.len() + 4 + 4 + data.len() + 4];
        put_u32_le(&mut buf, 0, idx);
        buf[4] = 2;
        put_u16_le(&mut buf, 5, name_bytes.len() as u16);
        buf[7..7 + name_bytes.len()].copy_from_slice(name_bytes);
        let mut pos = 7 + name_bytes.len();
        put_u32_le(&mut buf, pos, value_type); pos += 4;
        put_u32_le(&mut buf, pos, data.len() as u32); pos += 4;
        buf[pos..pos + data.len()].copy_from_slice(data); pos += data.len();
        put_u32_le(&mut buf, pos, next_val);
        self.cells.insert(idx, buf);

        let (type_name, display) = if value_type == REG_DWORD {
            let val = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            ("REG_DWORD", val.to_string())
        } else {
            let s = String::from_utf8_lossy(data).trim_end_matches('\0').to_string();
            ("REG_SZ", format!("'{}'", s))
        };
        self.values.insert(idx, ValueMeta { name: name.to_string(), type_name, display });
        self.key_values.entry(parent_key).or_default().push(idx);
    }

    fn serialize(&self) -> Vec<u8> {
        let entries: Vec<(&u32, &Vec<u8>)> = self.cells.iter().collect();
        let mut checksum: u32 = 0;

        for (_, data) in &entries {
            let cell_idx = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let cell_type = data[4] as u32;
            checksum = checksum.wrapping_add(cell_idx);
            checksum = checksum.wrapping_add(cell_type);
            let name_len = u16::from_le_bytes(data[5..7].try_into().unwrap());
            checksum = checksum.wrapping_add(name_len as u32);
            let mut pos = 7;
            for &b in &data[pos..pos + name_len as usize] {
                checksum = checksum.wrapping_add(b as u32);
            }
            pos += name_len as usize;
            if cell_type == 1 {
                for _ in 0..5 {
                    let v = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                    checksum = checksum.wrapping_add(v);
                    pos += 4;
                }
                let lw_low = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                let lw_high = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
                checksum = checksum.wrapping_add(lw_low);
                checksum = checksum.wrapping_add(lw_high);
            } else if cell_type == 2 {
                let value_type = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                checksum = checksum.wrapping_add(value_type);
                let data_len = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap());
                checksum = checksum.wrapping_add(data_len);
                pos += 8;
                for &b in &data[pos..pos + data_len as usize] {
                    checksum = checksum.wrapping_add(b as u32);
                }
                pos += data_len as usize;
                let nxt = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                checksum = checksum.wrapping_add(nxt);
            }
        }

        let mut result = Vec::with_capacity(16 + entries.len() * 64);
        let mut hdr = vec![0u8; 16];
        put_u32_le(&mut hdr, 0, NEOH_MAGIC);
        put_u32_le(&mut hdr, 4, NEOH_VERSION);
        put_u32_le(&mut hdr, 8, entries.len() as u32);
        put_u32_le(&mut hdr, 12, checksum);
        result.extend_from_slice(&hdr);
        for (_, data) in &entries {
            result.extend_from_slice(data);
        }
        result
    }

}

struct HiveResult {
    data: Vec<u8>,
    cell_count: usize,
    keys: BTreeMap<u32, KeyMeta>,
    values: BTreeMap<u32, ValueMeta>,
    key_values: BTreeMap<u32, Vec<u32>>,
}

fn build_default_system_hive(enable_tests: bool, enable_network_test: bool) -> HiveResult {
    let mut b = HiveBuilder::new();

    const _ROOT: u32 = 0;
    const _CCS: u32 = 1;
    const _SVC: u32 = 2;
    const _NEO: u32 = 3;
    const _DHCPC: u32 = 4;
    const _NET: u32 = 5;
    const _IFC: u32 = 6;
    const _IF0: u32 = 7;
    const _CTL: u32 = 8;
    const _V_SHELL: u32 = 9;
    const _V_VT: u32 = 10;
    const _V_AUTO: u32 = 11;
    const _V_TESTS: u32 = 12;
    const _V_DHCP: u32 = 13;
    const _V_WAIT: u32 = 14;
    const _V_DNAME: u32 = 15;
    const _V_BPATH: u32 = 16;
    const _V_IPATH: u32 = 17;
    const _V_STYPE: u32 = 18;
    const _V_RPOL: u32 = 19;
    const _V_MFAIL: u32 = 20;
    const _V_DEPS: u32 = 21;
    const _V_DESC: u32 = 22;
    const _V_BENCH: u32 = 23;
    const _V_AHCI: u32 = 24;
    const _LOC_KEY: u32 = 25;
    const _V_LANG: u32 = 26;
    const _SM: u32 = 27;
    const _ENV: u32 = 28;
    const _V_PATH: u32 = 29;
    const _PWR: u32 = 30;
    const _PLN: u32 = 31;
    const _BAL: u32 = 32;
    const _PER: u32 = 33;
    const _SAV: u32 = 34;
    const _V_AP: u32 = 35;
    const _V_BAL_DT: u32 = 36;
    const _V_BAL_ST: u32 = 37;
    const _V_BAL_HE: u32 = 38;
    const _V_BAL_CP: u32 = 39;
    const _V_BAL_LA: u32 = 40;
    const _V_BAL_PBA: u32 = 41;
    const _V_PER_DT: u32 = 42;
    const _V_PER_ST: u32 = 43;
    const _V_PER_HE: u32 = 44;
    const _V_PER_CP: u32 = 45;
    const _V_PER_LA: u32 = 46;
    const _V_PER_PBA: u32 = 47;
    const _V_SAV_DT: u32 = 48;
    const _V_SAV_ST: u32 = 49;
    const _V_SAV_HE: u32 = 50;
    const _V_SAV_CP: u32 = 51;
    const _V_SAV_LA: u32 = 52;
    const _V_SAV_PBA: u32 = 53;
    const _V_NETTEST: u32 = 54;
    const _COMP: u32 = 55;
    const _V_COMPNAME: u32 = 56;

    b.next_idx = 57;

    let tests_val: u32 = if enable_tests { 1 } else { 0 };
    let net_test_val: u32 = if enable_network_test { 1 } else { 0 };

    // NeoInit values chain: EnableNetworkTest → EnableTests → AutoStartServices → EnableVT → DefaultShell
    b.add_value(_V_SHELL, "DefaultShell", REG_SZ, b"C:\\Programs\\neoshell.nxe\0", NULL_CELL, _NEO);
    b.add_value(_V_VT, "EnableVT", REG_DWORD, &1u32.to_le_bytes(), _V_SHELL, _NEO);
    b.add_value(_V_AUTO, "AutoStartServices", REG_SZ, b"", _V_VT, _NEO);
    b.add_value(_V_TESTS, "EnableTests", REG_DWORD, &tests_val.to_le_bytes(), _V_AUTO, _NEO);
    b.add_value(_V_NETTEST, "EnableNetworkTest", REG_DWORD, &net_test_val.to_le_bytes(), _V_TESTS, _NEO);

    // Dhcpc values chain: DisplayName → BinaryPath → ImagePath → StartType → RestartPolicy → MaxFailures → Dependencies → Description
    b.add_value(_V_DESC, "Description", REG_SZ, b"DHCP Client Service\0", NULL_CELL, _DHCPC);
    b.add_value(_V_DEPS, "Dependencies", REG_SZ, b"", _V_DESC, _DHCPC);
    b.add_value(_V_MFAIL, "MaxFailures", REG_DWORD, &3u32.to_le_bytes(), _V_DEPS, _DHCPC);
    b.add_value(_V_RPOL, "RestartPolicy", REG_DWORD, &1u32.to_le_bytes(), _V_MFAIL, _DHCPC);
    b.add_value(_V_STYPE, "StartType", REG_DWORD, &2u32.to_le_bytes(), _V_RPOL, _DHCPC);
    b.add_value(_V_IPATH, "ImagePath", REG_SZ, b"C:\\System\\Tools\\dhcpd.nxe\0", _V_STYPE, _DHCPC);
    b.add_value(_V_BPATH, "BinaryPath", REG_SZ, b"C:\\System\\Tools\\dhcpd.nxe\0", _V_IPATH, _DHCPC);
    b.add_value(_V_DNAME, "DisplayName", REG_SZ, b"DHCP Client\0", _V_BPATH, _DHCPC);

    // Interfaces\0: DHCPEnabled
    b.add_value(_V_DHCP, "DHCPEnabled", REG_DWORD, &1u32.to_le_bytes(), NULL_CELL, _IF0);

    // Control values: WaitForNetwork → AhciDebug → BenchmarkReport
    b.add_value(_V_BENCH, "BenchmarkReport", REG_DWORD, &0u32.to_le_bytes(), NULL_CELL, _CTL);
    b.add_value(_V_AHCI, "AhciDebug", REG_DWORD, &0u32.to_le_bytes(), _V_BENCH, _CTL);
    b.add_value(_V_WAIT, "WaitForNetwork", REG_DWORD, &0u32.to_le_bytes(), _V_AHCI, _CTL);

    // Locale
    b.add_value(_V_LANG, "Language", REG_SZ, b"es-ES", NULL_CELL, _LOC_KEY);

    // Balanced chain: PowerButtonAction → LidAction → CpuPolicy → HibernateEnabled → SleepTimeout → DisplayTimeout
    b.add_value(_V_BAL_DT, "DisplayTimeout", REG_DWORD, &10u32.to_le_bytes(), NULL_CELL, _BAL);
    b.add_value(_V_BAL_ST, "SleepTimeout", REG_DWORD, &30u32.to_le_bytes(), _V_BAL_DT, _BAL);
    b.add_value(_V_BAL_HE, "HibernateEnabled", REG_DWORD, &1u32.to_le_bytes(), _V_BAL_ST, _BAL);
    b.add_value(_V_BAL_CP, "CpuPolicy", REG_SZ, b"Balanced", _V_BAL_HE, _BAL);
    b.add_value(_V_BAL_LA, "LidAction", REG_DWORD, &1u32.to_le_bytes(), _V_BAL_CP, _BAL);
    b.add_value(_V_BAL_PBA, "PowerButtonAction", REG_DWORD, &3u32.to_le_bytes(), _V_BAL_LA, _BAL);

    // Performance chain
    b.add_value(_V_PER_DT, "DisplayTimeout", REG_DWORD, &30u32.to_le_bytes(), NULL_CELL, _PER);
    b.add_value(_V_PER_ST, "SleepTimeout", REG_DWORD, &60u32.to_le_bytes(), _V_PER_DT, _PER);
    b.add_value(_V_PER_HE, "HibernateEnabled", REG_DWORD, &0u32.to_le_bytes(), _V_PER_ST, _PER);
    b.add_value(_V_PER_CP, "CpuPolicy", REG_SZ, b"Performance", _V_PER_HE, _PER);
    b.add_value(_V_PER_LA, "LidAction", REG_DWORD, &0u32.to_le_bytes(), _V_PER_CP, _PER);
    b.add_value(_V_PER_PBA, "PowerButtonAction", REG_DWORD, &3u32.to_le_bytes(), _V_PER_LA, _PER);

    // PowerSaver chain
    b.add_value(_V_SAV_DT, "DisplayTimeout", REG_DWORD, &3u32.to_le_bytes(), NULL_CELL, _SAV);
    b.add_value(_V_SAV_ST, "SleepTimeout", REG_DWORD, &10u32.to_le_bytes(), _V_SAV_DT, _SAV);
    b.add_value(_V_SAV_HE, "HibernateEnabled", REG_DWORD, &1u32.to_le_bytes(), _V_SAV_ST, _SAV);
    b.add_value(_V_SAV_CP, "CpuPolicy", REG_SZ, b"PowerSave", _V_SAV_HE, _SAV);
    b.add_value(_V_SAV_LA, "LidAction", REG_DWORD, &1u32.to_le_bytes(), _V_SAV_CP, _SAV);
    b.add_value(_V_SAV_PBA, "PowerButtonAction", REG_DWORD, &2u32.to_le_bytes(), _V_SAV_LA, _SAV);

    // ActivePlan + PATH + ComputerName
    b.add_value(_V_AP, "ActivePlan", REG_DWORD, &0u32.to_le_bytes(), NULL_CELL, _PWR);
    b.add_value(_V_PATH, "PATH", REG_SZ, b"\\Programs;\\System\\Tools\0", NULL_CELL, _ENV);
    b.add_value(_V_COMPNAME, "ComputerName", REG_SZ, b"NeoDOS-PC\0", NULL_CELL, _COMP);

    // Keys
    b.add_key(_NEO, "NeoInit", _SVC, NULL_CELL, _DHCPC, _V_NETTEST, NULL_CELL, 0);
    b.add_key(_DHCPC, "Dhcpc", _SVC, NULL_CELL, _NET, _V_DNAME, NULL_CELL, 0);
    b.add_key(_IF0, "0", _IFC, NULL_CELL, NULL_CELL, _V_DHCP, NULL_CELL, 0);
    b.add_key(_IFC, "Interfaces", _NET, _IF0, NULL_CELL, NULL_CELL, NULL_CELL, 0);
    b.add_key(_NET, "Network", _SVC, _IFC, NULL_CELL, NULL_CELL, NULL_CELL, 0);
    b.add_key(_COMP, "ComputerName", _CTL, NULL_CELL, _LOC_KEY, _V_COMPNAME, NULL_CELL, 0);
    b.add_key(_LOC_KEY, "Locale", _CTL, NULL_CELL, _SM, _V_LANG, NULL_CELL, 0);
    b.add_key(_ENV, "Environment", _SM, NULL_CELL, NULL_CELL, _V_PATH, NULL_CELL, 0);
    b.add_key(_SM, "Session Manager", _CTL, _ENV, NULL_CELL, NULL_CELL, NULL_CELL, 0);
    b.add_key(_CTL, "Control", _CCS, _COMP, NULL_CELL, _V_WAIT, NULL_CELL, 0);

    b.add_key(_PWR, "Power", _ROOT, _PLN, NULL_CELL, _V_AP, NULL_CELL, 0);
    b.add_key(_PLN, "Plans", _PWR, _BAL, NULL_CELL, NULL_CELL, NULL_CELL, 0);
    b.add_key(_BAL, "Balanced", _PLN, NULL_CELL, _PER, _V_BAL_PBA, NULL_CELL, 0);
    b.add_key(_PER, "Performance", _PLN, NULL_CELL, _SAV, _V_PER_PBA, NULL_CELL, 0);
    b.add_key(_SAV, "PowerSaver", _PLN, NULL_CELL, NULL_CELL, _V_SAV_PBA, NULL_CELL, 0);

    b.add_key(_SVC, "Services", _CCS, _NEO, _CTL, NULL_CELL, NULL_CELL, 0);
    b.add_key(_CCS, "CurrentControlSet", _ROOT, _SVC, _PWR, NULL_CELL, NULL_CELL, 0);
    b.add_key(_ROOT, "SYSTEM", NULL_CELL, _CCS, NULL_CELL, NULL_CELL, NULL_CELL, 0);

    let cell_count = b.cells.len();
    HiveResult {
        data: b.serialize(),
        cell_count,
        keys: b.keys,
        values: b.values,
        key_values: b.key_values,
    }
}

fn print_usage() {
    eprintln!("Usage: gen-hiv [options] [output_path]");
    eprintln!("Options:");
    eprintln!("  --enable-tests         Set EnableTests=1 (boot-time tests)");
    eprintln!("  --enable-network-test  Set EnableNetworkTest=1");
    eprintln!("Default output: scripts/system.hiv");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut output = PathBuf::from("scripts/system.hiv");
    let mut enable_tests = false;
    let mut enable_network_test = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => { print_usage(); return; }
            "--enable-tests" => enable_tests = true,
            "--enable-network-test" => enable_network_test = true,
            _ => {
                if !args[i].starts_with("--") {
                    output = PathBuf::from(&args[i]);
                } else {
                    eprintln!("Unknown option: {}", args[i]);
                    print_usage();
                    process::exit(1);
                }
            }
        }
        i += 1;
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).ok();
    }

    let result = build_default_system_hive(enable_tests, enable_network_test);
    fs::write(&output, &result.data).unwrap_or_else(|e| {
        eprintln!("Error writing {}: {}", output.display(), e);
        process::exit(1);
    });

    println!("Generated {} ({} bytes, NEOHv1)", output.display(), result.data.len());
    println!("  Cells: {}", result.cell_count);
    println!("  Root key: SYSTEM");

    // Build path cache
    fn build_path(idx: u32, keys: &BTreeMap<u32, KeyMeta>, cache: &mut BTreeMap<u32, String>) -> String {
        if let Some(path) = cache.get(&idx) {
            return path.clone();
        }
        let name = &keys[&idx].name;
        let parent = keys[&idx].parent;
        let path = if parent == NULL_CELL {
            name.clone()
        } else {
            format!("{}\\{}", build_path(parent, keys, cache), name)
        };
        cache.insert(idx, path.clone());
        path
    }

    let mut path_cache = BTreeMap::new();
    let tests_val = if enable_tests { "1" } else { "0" };
    let net_test_val = if enable_network_test { "1" } else { "0" };

    println!("  Values:");
    for (key_id, v_ids) in &result.key_values {
        let path = build_path(*key_id, &result.keys, &mut path_cache);
        for vid in v_ids {
            let v = &result.values[vid];
            let display = match v.name.as_str() {
                "EnableTests" => tests_val.to_string(),
                "EnableNetworkTest" => net_test_val.to_string(),
                _ => v.display.clone(),
            };
            println!("    {}\\{} = {} ({})", path, v.name, display, v.type_name);
        }
    }
}
