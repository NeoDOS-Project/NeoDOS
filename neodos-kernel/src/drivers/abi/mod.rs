use crate::nem::{ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbiVersion {
    pub min: u16,
    pub target: u16,
    pub max: u16,
}

impl AbiVersion {
    pub const fn new(min: u16, target: u16, max: u16) -> Self {
        Self { min, target, max }
    }

    pub fn from_header(abi_min: u16, abi_target: u16, abi_max: u16) -> Self {
        Self::new(abi_min, abi_target, abi_max)
    }

    pub fn kernel_default() -> Self {
        Self::new(ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID)
    }

    pub fn is_valid(&self) -> bool {
        self.min > 0 && self.target > 0 && self.max > 0
            && self.min <= self.target && self.target <= self.max
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiationResult {
    Compatible,
    CompatibleWithWarnings(&'static [&'static str]),
    Incompatible(&'static str),
}

impl NegotiationResult {
    pub fn is_compatible(&self) -> bool {
        matches!(self, NegotiationResult::Compatible | NegotiationResult::CompatibleWithWarnings(_))
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            NegotiationResult::Compatible => "Compatible",
            NegotiationResult::CompatibleWithWarnings(w) => w.first().unwrap_or(&"Compatible with warnings"),
            NegotiationResult::Incompatible(r) => r,
        }
    }
}

pub fn negotiate(kernel: &AbiVersion, driver: &AbiVersion) -> NegotiationResult {
    if !kernel.is_valid() {
        return NegotiationResult::Incompatible("Kernel ABI version is invalid");
    }
    if !driver.is_valid() {
        return NegotiationResult::Incompatible("Driver ABI fields are invalid");
    }

    if driver.min > kernel.max {
        return NegotiationResult::Incompatible("Driver requires newer ABI than kernel supports");
    }
    if driver.max < kernel.min {
        return NegotiationResult::Incompatible("Driver ABI is too old for this kernel");
    }
    if driver.target < kernel.min || driver.target > kernel.max {
        return NegotiationResult::Incompatible("Driver ABI target is outside kernel's supported range");
    }

    let mut warnings: &[&str] = &[];
    if driver.max < kernel.target {
        warnings = &["Driver ABI predates kernel target — some features may be unavailable"];
    } else if driver.target > kernel.target {
        warnings = &["Driver targets a newer ABI than kernel default — using compatibility mode"];
    }

    if warnings.is_empty() {
        NegotiationResult::Compatible
    } else {
        NegotiationResult::CompatibleWithWarnings(warnings)
    }
}

pub fn negotiate_default(driver_min: u16, driver_target: u16, driver_max: u16) -> NegotiationResult {
    let kernel = AbiVersion::kernel_default();
    let driver = AbiVersion::from_header(driver_min, driver_target, driver_max);
    negotiate(&kernel, &driver)
}

pub fn register_abi_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("abi_kernel_default_valid", {
        let k = AbiVersion::kernel_default();
        test_true!(k.is_valid());
        test_eq!(k.min, ABI_MIN_VALID);
        test_eq!(k.target, ABI_TARGET);
        test_eq!(k.max, ABI_MAX_VALID);
    });

    test_case!("abi_driver_valid_accept", {
        let k = AbiVersion::kernel_default();
        let d = AbiVersion::new(1, 1, 2);
        let r = negotiate(&k, &d);
        test_true!(r.is_compatible());
        test_eq!(r, NegotiationResult::Compatible);
    });

    test_case!("abi_driver_too_new", {
        let k = AbiVersion::kernel_default();
        let d = AbiVersion::new(3, 3, 4);
        let r = negotiate(&k, &d);
        test_eq!(r.is_compatible(), false);
        test_eq!(r.to_str(), "Driver requires newer ABI than kernel supports");
    });

    test_case!("abi_driver_too_old", {
        let k = AbiVersion::kernel_default();
        let d = AbiVersion::new(0, 0, 0);
        let r = negotiate(&k, &d);
        test_eq!(r.is_compatible(), false);
    });

    test_case!("abi_driver_zero_min", {
        let k = AbiVersion::kernel_default();
        let d = AbiVersion::new(0, 1, 2);
        let r = negotiate(&k, &d);
        test_eq!(r.is_compatible(), false);
        test_eq!(r.to_str(), "Driver ABI fields are invalid");
    });

    test_case!("abi_driver_out_of_order", {
        let k = AbiVersion::kernel_default();
        let d = AbiVersion::new(3, 2, 4);
        let r = negotiate(&k, &d);
        test_eq!(r.is_compatible(), false);
    });

    test_case!("abi_driver_older_max_warning", {
        let k = AbiVersion::new(1, 2, 3);
        let d = AbiVersion::new(1, 1, 1);
        let r = negotiate(&k, &d);
        test_true!(r.is_compatible());
        match r {
            NegotiationResult::CompatibleWithWarnings(_) => {},
            _ => test_true!(false),
        }
    });

    test_case!("abi_driver_target_newer_warning", {
        let k = AbiVersion::new(1, 1, 3);
        let d = AbiVersion::new(1, 2, 3);
        let r = negotiate(&k, &d);
        test_true!(r.is_compatible());
        match r {
            NegotiationResult::CompatibleWithWarnings(_) => {},
            _ => test_true!(false),
        }
    });

    test_case!("abi_negotiate_default_compatible", {
        let r = negotiate_default(ABI_MIN_VALID, ABI_TARGET, ABI_MAX_VALID);
        test_true!(r.is_compatible());
    });

    test_case!("abi_negotiate_default_incompatible", {
        let r = negotiate_default(99, 99, 100);
        test_eq!(r.is_compatible(), false);
    });
}
