use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;

const MAX_DEPS: usize = 32;
const MAX_DRIVERS_DEPS: usize = 16;

#[derive(Debug, Clone)]
pub struct DriverDependency {
    pub driver: String,
    pub depends_on: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepError {
    CircularDependency,
    MissingDependency(&'static str),
    TooManyDependencies,
    DriverNotFound,
}

pub struct DependencyGraph {
    edges: BTreeMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            edges: BTreeMap::new(),
        }
    }

    pub fn add_driver(&mut self, name: &str) {
        self.edges.entry(name.to_ascii_uppercase()).or_insert_with(Vec::new);
    }

    pub fn add_dependency(&mut self, driver: &str, depends_on: &str) -> Result<(), DepError> {
        let driver_upper = driver.to_ascii_uppercase();
        let dep_upper = depends_on.to_ascii_uppercase();

        if !self.edges.contains_key(&driver_upper) {
            self.edges.entry(driver_upper.clone()).or_insert_with(Vec::new);
        }

        if !self.edges.contains_key(&dep_upper) {
            self.edges.entry(dep_upper.clone()).or_insert_with(Vec::new);
        }

        let deps = self.edges.get_mut(&driver_upper).unwrap();
        if deps.len() >= MAX_DEPS {
            return Err(DepError::TooManyDependencies);
        }
        if !deps.contains(&dep_upper) {
            deps.push(dep_upper);
        }
        Ok(())
    }

    pub fn has_cycle(&self) -> bool {
        let mut visited = Vec::new();
        let mut in_stack = Vec::new();

        for name in self.edges.keys() {
            if self.dfs_cycle(name, &mut visited, &mut in_stack) {
                return true;
            }
        }
        false
    }

    fn dfs_cycle(&self, node: &str, visited: &mut Vec<String>, in_stack: &mut Vec<String>) -> bool {
        if in_stack.contains(&node.to_string()) {
            return true;
        }
        if visited.contains(&node.to_string()) {
            return false;
        }

        visited.push(node.to_string());
        in_stack.push(node.to_string());

        if let Some(deps) = self.edges.get(node) {
            for dep in deps {
                if self.dfs_cycle(dep, visited, in_stack) {
                    return true;
                }
            }
        }

        in_stack.pop();
        false
    }

    pub fn resolve_order(&self) -> Result<Vec<String>, DepError> {
        if self.has_cycle() {
            return Err(DepError::CircularDependency);
        }

        let mut visited = Vec::new();
        let mut result = Vec::new();

        let all_names: Vec<String> = self.edges.keys().cloned().collect();
        for name in &all_names {
            if !visited.contains(name) {
                self.dfs_topological(name, &mut visited, &mut result)?;
            }
        }

        Ok(result)
    }

    fn dfs_topological(
        &self,
        node: &str,
        visited: &mut Vec<String>,
        result: &mut Vec<String>,
    ) -> Result<(), DepError> {
        if visited.contains(&node.to_string()) {
            return Ok(());
        }

        visited.push(node.to_string());

        if let Some(deps) = self.edges.get(node) {
            for dep in deps {
                self.dfs_topological(dep, visited, result)?;
            }
        }

        result.push(node.to_string());
        Ok(())
    }

    pub fn clear(&mut self) {
        self.edges.clear();
    }

    pub fn driver_count(&self) -> usize {
        self.edges.len()
    }

    pub fn all_drivers(&self) -> Vec<String> {
        self.edges.keys().cloned().collect()
    }

    pub fn dependencies_of(&self, driver: &str) -> Vec<String> {
        self.edges.get(&driver.to_ascii_uppercase())
            .cloned()
            .unwrap_or_default()
    }
}

pub fn resolve_nem_symbol_dependencies(
    symbols: &[crate::nem::NemSymbol],
    strtab: &[u8],
) -> Vec<String> {
    let mut deps = Vec::new();
    for sym in symbols {
        let off = sym.name_off as usize;
        if off >= strtab.len() { continue; }
        let end = match strtab[off..].iter().position(|&b| b == 0) {
            Some(e) => e,
            None => continue,
        };
        let name = match core::str::from_utf8(&strtab[off..off + end]) {
            Ok(n) => n,
            Err(_) => continue,
        };
        if let Some(dep_name) = name.strip_prefix("__dep_") {
            if !dep_name.is_empty() {
                deps.push(dep_name.to_ascii_uppercase());
            }
        }
    }
    deps
}

pub fn register_dependency_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("dep_graph_empty", {
        let g = DependencyGraph::new();
        test_eq!(g.driver_count(), 0);
        test_eq!(g.has_cycle(), false);
        let order = g.resolve_order().unwrap();
        test_eq!(order.len(), 0);
    });

    test_case!("dep_graph_single_driver", {
        let mut g = DependencyGraph::new();
        g.add_driver("PS2KBD");
        test_eq!(g.driver_count(), 1);
        test_eq!(g.has_cycle(), false);
        let order = g.resolve_order().unwrap();
        test_eq!(order.len(), 1);
        test_eq!(order[0], "PS2KBD");
    });

    test_case!("dep_graph_simple_order", {
        let mut g = DependencyGraph::new();
        g.add_driver("PS2KBD");
        g.add_driver("SERIAL");
        g.add_driver("KBD_MUX");
        g.add_dependency("KBD_MUX", "PS2KBD").unwrap();
        let order = g.resolve_order().unwrap();
        test_eq!(order.len(), 3);
        let ps2_pos = order.iter().position(|n| n == "PS2KBD").unwrap();
        let mux_pos = order.iter().position(|n| n == "KBD_MUX").unwrap();
        test_true!(ps2_pos < mux_pos);
    });

    test_case!("dep_graph_chain_order", {
        let mut g = DependencyGraph::new();
        g.add_driver("A");
        g.add_driver("B");
        g.add_driver("C");
        g.add_dependency("C", "B").unwrap();
        g.add_dependency("B", "A").unwrap();
        let order = g.resolve_order().unwrap();
        let a_pos = order.iter().position(|n| n == "A").unwrap();
        let b_pos = order.iter().position(|n| n == "B").unwrap();
        let c_pos = order.iter().position(|n| n == "C").unwrap();
        test_true!(a_pos < b_pos);
        test_true!(b_pos < c_pos);
    });

    test_case!("dep_graph_cycle_detection", {
        let mut g = DependencyGraph::new();
        g.add_driver("A");
        g.add_driver("B");
        g.add_dependency("A", "B").unwrap();
        g.add_dependency("B", "A").unwrap();
        test_eq!(g.has_cycle(), true);
        test_eq!(g.resolve_order().is_err(), true);
    });

    test_case!("dep_graph_missing_dependency", {
        let mut g = DependencyGraph::new();
        g.add_driver("DRV1");
        g.add_dependency("DRV1", "MISSING_DRV").unwrap();
        let order = g.resolve_order().unwrap();
        test_true!(order.len() >= 2);
        test_true!(order.contains(&"DRV1".to_string()));
        test_true!(order.contains(&"MISSING_DRV".to_string()));
    });

    test_case!("dep_graph_diamond_deps", {
        let mut g = DependencyGraph::new();
        g.add_driver("BASE");
        g.add_driver("LEFT");
        g.add_driver("RIGHT");
        g.add_driver("TOP");
        g.add_dependency("LEFT", "BASE").unwrap();
        g.add_dependency("RIGHT", "BASE").unwrap();
        g.add_dependency("TOP", "LEFT").unwrap();
        g.add_dependency("TOP", "RIGHT").unwrap();
        test_eq!(g.has_cycle(), false);
        let order = g.resolve_order().unwrap();
        test_eq!(order.len(), 4);
        let base_pos = order.iter().position(|n| n == "BASE").unwrap();
        let top_pos = order.iter().position(|n| n == "TOP").unwrap();
        test_true!(base_pos < top_pos);
    });

    test_case!("dep_graph_clear", {
        let mut g = DependencyGraph::new();
        g.add_driver("A");
        g.clear();
        test_eq!(g.driver_count(), 0);
    });

    test_case!("dep_symbol_extract", {
        use crate::nem::NemSymbol;
        let strtab = b"__dep_PS2KBD\0__dep_SERIAL\0other_sym\0";
        let symbols = [
            NemSymbol { name_off: 0, value: 0, section: 0xFFFF, info: 0, _pad1: 0, _pad2: 0 },
            NemSymbol { name_off: 13, value: 0, section: 0xFFFF, info: 0, _pad1: 0, _pad2: 0 },
            NemSymbol { name_off: 26, value: 0, section: 0xFFFF, info: 0, _pad1: 0, _pad2: 0 },
        ];
        let deps = resolve_nem_symbol_dependencies(&symbols, strtab);
        test_eq!(deps.len(), 2);
        test_true!(deps.contains(&"PS2KBD".to_string()));
        test_true!(deps.contains(&"SERIAL".to_string()));
    });

    test_case!("dep_symbol_empty", {
        use crate::nem::NemSymbol;
        let strtab = b"any_sym\0another\0";
        let symbols = [
            NemSymbol { name_off: 0, value: 0, section: 0xFFFF, info: 0, _pad1: 0, _pad2: 0 },
        ];
        let deps = resolve_nem_symbol_dependencies(&symbols, strtab);
        test_eq!(deps.len(), 0);
    });

    test_case!("dep_case_insensitive", {
        let mut g = DependencyGraph::new();
        g.add_driver("PS2KBD");
        g.add_dependency("kbd_mux", "ps2kbd").unwrap();
        let deps = g.dependencies_of("KBD_MUX");
        test_eq!(deps.len(), 1);
        test_eq!(deps[0], "PS2KBD");
    });

    test_case!("dep_multiple_drivers_order", {
        let mut g = DependencyGraph::new();
        g.add_driver("ATA");
        g.add_driver("PCI");
        g.add_driver("NVME");
        g.add_driver("AHCI");
        g.add_dependency("ATA", "PCI").unwrap();
        g.add_dependency("NVME", "PCI").unwrap();
        g.add_dependency("AHCI", "PCI").unwrap();
        test_eq!(g.has_cycle(), false);
        let order = g.resolve_order().unwrap();
        test_eq!(order.len(), 4);
        let pci_pos = order.iter().position(|n| n == "PCI").unwrap();
        let ahci_pos = order.iter().position(|n| n == "AHCI").unwrap();
        let ata_pos = order.iter().position(|n| n == "ATA").unwrap();
        let nvme_pos = order.iter().position(|n| n == "NVME").unwrap();
        test_true!(pci_pos < ahci_pos);
        test_true!(pci_pos < ata_pos);
        test_true!(pci_pos < nvme_pos);
    });

    test_case!("dep_self_cycle", {
        let mut g = DependencyGraph::new();
        g.add_dependency("A", "A").unwrap();
        test_eq!(g.has_cycle(), true);
    });
}
