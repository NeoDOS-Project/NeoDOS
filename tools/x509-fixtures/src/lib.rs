//! Host-only X.509 fixtures for `libtrust` / `libtls` tests.
//!
//! Nothing is committed: every certificate is generated once per test process
//! with OpenSSL and fixed validity dates. A missing `openssl` or a failed
//! command is a test failure, never a skipped test.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// 2027-01-15T08:00:00Z; inside every "valid" fixture.
pub const NOW: i64 = 1_800_000_000;

const VALID_NB: &str = "20250101000000Z";
const VALID_NA: &str = "20350101000000Z";
const EXPIRED_NB: &str = "20150101000000Z";
const EXPIRED_NA: &str = "20200101000000Z";
const FUTURE_NB: &str = "20300101000000Z";
const FUTURE_NA: &str = "20400101000000Z";

/// DER certificates used by the host tests.
pub struct Fixtures {
    pub ca_rsa: Vec<u8>,
    pub ca_rsa_pem: Vec<u8>,
    pub ca_pss: Vec<u8>,
    pub ca_p256: Vec<u8>,
    pub ca_p384: Vec<u8>,
    pub ca_no_key_usage: Vec<u8>,
    pub ca_bad_key_usage: Vec<u8>,
    pub ca_expired: Vec<u8>,
    pub ca_not_yet_valid: Vec<u8>,
    pub ca_other_rsa: Vec<u8>,
    pub ca_at_limit: Vec<u8>,
    pub ca_over_limit: Vec<u8>,
    pub intermediate_pathlen0: Vec<u8>,
    pub intermediate_pathlen1: Vec<u8>,
    pub intermediate_pathlen300: Vec<u8>,
    pub intermediate_pathlen0_layer: Vec<u8>,
    pub intermediate_pathlen1_layer: Vec<u8>,
    pub intermediate_bad_ku: Vec<u8>,
    pub leaf_rsa: Vec<u8>,
    pub leaf_pss: Vec<u8>,
    pub leaf_p256: Vec<u8>,
    pub leaf_p384: Vec<u8>,
    pub leaf_wrong_host: Vec<u8>,
    pub leaf_expired: Vec<u8>,
    pub leaf_not_yet: Vec<u8>,
    pub leaf_expired_root: Vec<u8>,
    pub leaf_valid_expired_root: Vec<u8>,
    pub leaf_other_ca: Vec<u8>,
    pub leaf_bad_ku: Vec<u8>,
    pub leaf_via_intermediate: Vec<u8>,
    pub leaf_via_bad_ku: Vec<u8>,
    pub leaf_pathlen0: Vec<u8>,
    pub leaf_pathlen300: Vec<u8>,
    pub leaf_pathlen_violation: Vec<u8>,
}

/// Generate (once) and return the fixtures.
pub fn fixtures() -> &'static Fixtures {
    static FIXTURES: OnceLock<Fixtures> = OnceLock::new();
    FIXTURES.get_or_init(|| generate().expect("X.509 fixtures"))
}

struct Workspace {
    dir: PathBuf,
    serial: u32,
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Workspace {
    fn new() -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "neodos-x509-fixtures-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, serial: 1000 })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn next_serial(&mut self) -> u32 {
        self.serial += 1;
        self.serial
    }

    fn write(&self, name: &str, text: &str) -> std::io::Result<()> {
        std::fs::write(self.path(name), text)
    }

    fn openssl<S: AsRef<OsStr> + std::fmt::Debug>(&self, args: &[S]) {
        let out = Command::new("openssl")
            .args(args)
            .output()
            .expect("openssl must be installed for the X.509 host tests");
        assert!(
            out.status.success(),
            "openssl {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn genrsa(&self, name: &str) {
        self.openssl(&[
            "genrsa",
            "-out",
            self.path(name).to_str().unwrap(),
            "2048",
        ]);
    }

    fn genec(&self, name: &str, curve: &str) {
        self.openssl(&[
            "ecparam",
            "-name",
            curve,
            "-genkey",
            "-noout",
            "-out",
            self.path(name).to_str().unwrap(),
        ]);
    }

    fn der(&self, name: &str) -> Vec<u8> {
        let der = self.path(&format!("{name}.der"));
        self.openssl(&[
            "x509",
            "-in",
            self.path(&format!("{name}.pem")).to_str().unwrap(),
            "-outform",
            "DER",
            "-out",
            der.to_str().unwrap(),
        ]);
        std::fs::read(&der).expect("read generated DER")
    }

    fn selfsigned_rsa(&self, name: &str, subject: &str, nb: &str, na: &str) {
        self.genrsa(&format!("{name}.key"));
        self.openssl(&[
            "req",
            "-x509",
            "-new",
            "-key",
            self.path(&format!("{name}.key")).to_str().unwrap(),
            "-sha256",
            "-not_before",
            nb,
            "-not_after",
            na,
            "-subj",
            subject,
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-out",
            self.path(&format!("{name}.pem")).to_str().unwrap(),
        ]);
    }

    fn selfsigned_ec(&self, name: &str, subject: &str, curve: &str, hash: &str) {
        self.genec(&format!("{name}.key"), curve);
        self.openssl(&[
            "req",
            "-x509",
            "-new",
            "-key",
            self.path(&format!("{name}.key")).to_str().unwrap(),
            hash,
            "-not_before",
            VALID_NB,
            "-not_after",
            VALID_NA,
            "-subj",
            subject,
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-out",
            self.path(&format!("{name}.pem")).to_str().unwrap(),
        ]);
    }

    fn csr(&self, name: &str, subject: &str) {
        self.openssl(&[
            "req",
            "-new",
            "-key",
            self.path(&format!("{name}.key")).to_str().unwrap(),
            "-subj",
            subject,
            "-out",
            self.path(&format!("{name}.csr")).to_str().unwrap(),
        ]);
    }

    fn sign(
        &mut self,
        name: &str,
        ca: &str,
        hash: &str,
        nb: &str,
        na: &str,
        ext: &str,
        pss: bool,
    ) {
        let serial = self.next_serial().to_string();
        let mut args: Vec<String> = vec![
            "x509".into(),
            "-req".into(),
            "-in".into(),
            self.path(&format!("{name}.csr")).to_string_lossy().into_owned(),
            "-CA".into(),
            self.path(&format!("{ca}.pem")).to_string_lossy().into_owned(),
            "-CAkey".into(),
            self.path(&format!("{ca}.key")).to_string_lossy().into_owned(),
            "-set_serial".into(),
            serial,
            hash.into(),
            "-not_before".into(),
            nb.into(),
            "-not_after".into(),
            na.into(),
            "-extfile".into(),
            self.path(ext).to_string_lossy().into_owned(),
            "-out".into(),
            self.path(&format!("{name}.pem")).to_string_lossy().into_owned(),
        ];
        if pss {
            args.extend_from_slice(&[
                "-sigopt".into(),
                "rsa_padding_mode:pss".into(),
                "-sigopt".into(),
                "rsa_pss_saltlen:32".into(),
            ]);
        }
        self.openssl(&args);
    }

    fn padded_ca(&self, target: usize) -> Vec<u8> {
        // The base certificate is ~817 bytes; pad with a non-critical private
        // extension. Adjust one byte at a time and let DER length headers settle.
        let mut n: isize = target as isize - 830;
        for _ in 0..32 {
            let n_usize = n.max(1) as usize;
            let mut der = String::with_capacity(n_usize * 3);
            for i in 0..n_usize {
                if i > 0 {
                    der.push(':');
                }
                der.push_str("00");
            }
            let ext = format!("1.2.3.4.5=DER:{der}");
            self.openssl(&[
                "req",
                "-x509",
                "-new",
                "-key",
                self.path("ca_rsa_sha256.key").to_str().unwrap(),
                "-sha256",
                "-not_before",
                VALID_NB,
                "-not_after",
                VALID_NA,
                "-subj",
                "/CN=NeoDOS Test Padded CA/O=NeoDOS Tests/C=ES",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
                "-addext",
                ext.as_str(),
                "-out",
                self.path("ca_padded.pem").to_str().unwrap(),
            ]);
            let bytes = self.der("ca_padded");
            if bytes.len() == target {
                return bytes;
            }
            n += target as isize - bytes.len() as isize;
        }
        panic!("could not generate a {target}-byte certificate");
    }
}

fn generate() -> Result<Fixtures, Box<dyn std::error::Error>> {
    let mut w = Workspace::new()?;

    w.write(
        "leaf.ext",
        "basicConstraints=critical,CA:FALSE\n\
         keyUsage=critical,digitalSignature\n\
         extendedKeyUsage=serverAuth\n\
         subjectAltName=DNS:test.homelab.arpa\n\
         subjectKeyIdentifier=hash\n\
         authorityKeyIdentifier=keyid:always\n",
    )?;
    w.write(
        "leaf_wrong_host.ext",
        "basicConstraints=critical,CA:FALSE\n\
         keyUsage=critical,digitalSignature\n\
         extendedKeyUsage=serverAuth\n\
         subjectAltName=DNS:other.example\n\
         subjectKeyIdentifier=hash\n\
         authorityKeyIdentifier=keyid:always\n",
    )?;
    w.write(
        "ca_pathlen0.ext",
        "basicConstraints=critical,CA:TRUE,pathlen:0\n\
         keyUsage=critical,keyCertSign,cRLSign\n\
         subjectKeyIdentifier=hash\n\
         authorityKeyIdentifier=keyid:always\n",
    )?;
    w.write(
        "ca_pathlen1.ext",
        "basicConstraints=critical,CA:TRUE,pathlen:1\n\
         keyUsage=critical,keyCertSign,cRLSign\n\
         subjectKeyIdentifier=hash\n\
         authorityKeyIdentifier=keyid:always\n",
    )?;
    w.write(
        "ca_pathlen300.ext",
        "basicConstraints=critical,CA:TRUE,pathlen:300\n\
         keyUsage=critical,keyCertSign,cRLSign\n\
         subjectKeyIdentifier=hash\n\
         authorityKeyIdentifier=keyid:always\n",
    )?;
    w.write(
        "ca_bad_ku.ext",
        "basicConstraints=critical,CA:TRUE,pathlen:1\n\
         keyUsage=critical,digitalSignature\n\
         subjectKeyIdentifier=hash\n\
         authorityKeyIdentifier=keyid:always\n",
    )?;

    // Roots.
    w.selfsigned_rsa(
        "ca_rsa_sha256",
        "/CN=NeoDOS Test RSA Root/O=NeoDOS Tests/C=ES",
        VALID_NB,
        VALID_NA,
    );
    w.selfsigned_rsa(
        "ca_other_rsa",
        "/CN=NeoDOS Other RSA Root/O=NeoDOS Tests/C=ES",
        VALID_NB,
        VALID_NA,
    );
    w.selfsigned_rsa(
        "ca_expired",
        "/CN=NeoDOS Test Expired CA/O=NeoDOS Tests/C=ES",
        EXPIRED_NB,
        EXPIRED_NA,
    );
    w.selfsigned_rsa(
        "ca_not_yet_valid",
        "/CN=NeoDOS Test Future CA/O=NeoDOS Tests/C=ES",
        FUTURE_NB,
        FUTURE_NA,
    );
    w.selfsigned_ec(
        "ca_p256",
        "/CN=NeoDOS Test P-256 Root/O=NeoDOS Tests/C=ES",
        "prime256v1",
        "-sha256",
    );
    w.selfsigned_ec(
        "ca_p384",
        "/CN=NeoDOS Test P-384 Root/O=NeoDOS Tests/C=ES",
        "secp384r1",
        "-sha384",
    );

    // CA without keyUsage.
    w.genrsa("ca_no_key_usage.key");
    w.openssl(&[
        "req",
        "-x509",
        "-new",
        "-key",
        w.path("ca_no_key_usage.key").to_str().unwrap(),
        "-sha256",
        "-not_before",
        VALID_NB,
        "-not_after",
        VALID_NA,
        "-subj",
        "/CN=NeoDOS Test No KeyUsage CA/O=NeoDOS Tests/C=ES",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-out",
        w.path("ca_no_key_usage.pem").to_str().unwrap(),
    ]);

    // CA whose keyUsage forbids keyCertSign.
    w.genrsa("ca_bad_key_usage.key");
    w.openssl(&[
        "req",
        "-x509",
        "-new",
        "-key",
        w.path("ca_bad_key_usage.key").to_str().unwrap(),
        "-sha256",
        "-not_before",
        VALID_NB,
        "-not_after",
        VALID_NA,
        "-subj",
        "/CN=NeoDOS Test Bad KeyUsage CA/O=NeoDOS Tests/C=ES",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-addext",
        "keyUsage=critical,digitalSignature",
        "-out",
        w.path("ca_bad_key_usage.pem").to_str().unwrap(),
    ]);

    // RSA-PSS root.
    w.genrsa("ca_rsa_pss.key");
    w.openssl(&[
        "req",
        "-x509",
        "-new",
        "-key",
        w.path("ca_rsa_pss.key").to_str().unwrap(),
        "-sha256",
        "-sigopt",
        "rsa_padding_mode:pss",
        "-sigopt",
        "rsa_pss_saltlen:32",
        "-not_before",
        VALID_NB,
        "-not_after",
        VALID_NA,
        "-subj",
        "/CN=NeoDOS Test RSA-PSS Root/O=NeoDOS Tests/C=ES",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
        "-out",
        w.path("ca_rsa_pss.pem").to_str().unwrap(),
    ]);

    // Direct leaves.
    let leaf_subject = "/CN=test.homelab.arpa/O=NeoDOS Tests/C=ES";
    let wrong_subject = "/CN=other.example/O=NeoDOS Tests/C=ES";
    w.genrsa("leaf_rsa.key");
    w.csr("leaf_rsa", leaf_subject);
    w.sign("leaf_rsa", "ca_rsa_sha256", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_wrong_host.key");
    w.csr("leaf_rsa_wrong_host", wrong_subject);
    w.sign("leaf_rsa_wrong_host", "ca_rsa_sha256", "-sha256", VALID_NB, VALID_NA, "leaf_wrong_host.ext", false);

    w.genrsa("leaf_rsa_expired.key");
    w.csr("leaf_rsa_expired", leaf_subject);
    w.sign("leaf_rsa_expired", "ca_rsa_sha256", "-sha256", EXPIRED_NB, EXPIRED_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_not_yet.key");
    w.csr("leaf_rsa_not_yet", leaf_subject);
    w.sign("leaf_rsa_not_yet", "ca_rsa_sha256", "-sha256", FUTURE_NB, FUTURE_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_other_ca.key");
    w.csr("leaf_rsa_other_ca", leaf_subject);
    w.sign("leaf_rsa_other_ca", "ca_other_rsa", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_expired_root.key");
    w.csr("leaf_rsa_expired_root", leaf_subject);
    w.sign("leaf_rsa_expired_root", "ca_expired", "-sha256", EXPIRED_NB, EXPIRED_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_valid_expired_root.key");
    w.csr("leaf_rsa_valid_expired_root", leaf_subject);
    w.sign("leaf_rsa_valid_expired_root", "ca_expired", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_bad_ku.key");
    w.csr("leaf_rsa_bad_ku", leaf_subject);
    w.sign("leaf_rsa_bad_ku", "ca_bad_key_usage", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    // P-256 / P-384 leaves.
    w.genec("leaf_p256.key", "prime256v1");
    w.csr("leaf_p256", leaf_subject);
    w.sign("leaf_p256", "ca_p256", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    w.genec("leaf_p384.key", "secp384r1");
    w.csr("leaf_p384", leaf_subject);
    w.sign("leaf_p384", "ca_p384", "-sha384", VALID_NB, VALID_NA, "leaf.ext", false);

    // RSA-PSS leaf.
    w.genrsa("leaf_rsa_pss.key");
    w.csr("leaf_rsa_pss", leaf_subject);
    w.sign("leaf_rsa_pss", "ca_rsa_pss", "-sha256", VALID_NB, VALID_NA, "leaf.ext", true);

    // Intermediate chains.
    for (name, ext) in [
        ("intermediate_pathlen1", "ca_pathlen1.ext"),
        ("intermediate_pathlen0", "ca_pathlen0.ext"),
        ("intermediate_pathlen300", "ca_pathlen300.ext"),
    ] {
        w.genrsa(&format!("{name}.key"));
        w.csr(name, "/CN=NeoDOS Test Intermediate/O=NeoDOS Tests/C=ES");
        w.sign(name, "ca_rsa_sha256", "-sha256", VALID_NB, VALID_NA, ext, false);
    }

    w.genrsa("leaf_rsa_via_intermediate.key");
    w.csr("leaf_rsa_via_intermediate", leaf_subject);
    w.sign("leaf_rsa_via_intermediate", "intermediate_pathlen1", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_pathlen0.key");
    w.csr("leaf_rsa_pathlen0", leaf_subject);
    w.sign("leaf_rsa_pathlen0", "intermediate_pathlen0", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    w.genrsa("leaf_rsa_pathlen300.key");
    w.csr("leaf_rsa_pathlen300", leaf_subject);
    w.sign("leaf_rsa_pathlen300", "intermediate_pathlen300", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    // Violation: int1 pathLen0 signs int2, which signs the leaf.
    w.genrsa("intermediate_pathlen0_layer.key");
    w.csr("intermediate_pathlen0_layer", "/CN=NeoDOS Test Int1 pathlen0/O=NeoDOS Tests/C=ES");
    w.sign("intermediate_pathlen0_layer", "ca_rsa_sha256", "-sha256", VALID_NB, VALID_NA, "ca_pathlen0.ext", false);

    w.genrsa("intermediate_pathlen1_layer.key");
    w.csr("intermediate_pathlen1_layer", "/CN=NeoDOS Test Int2 pathlen1/O=NeoDOS Tests/C=ES");
    w.sign("intermediate_pathlen1_layer", "intermediate_pathlen0_layer", "-sha256", VALID_NB, VALID_NA, "ca_pathlen1.ext", false);

    w.genrsa("leaf_rsa_pathlen_violation.key");
    w.csr("leaf_rsa_pathlen_violation", leaf_subject);
    w.sign("leaf_rsa_pathlen_violation", "intermediate_pathlen1_layer", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    // Intermediate without keyCertSign.
    w.genrsa("intermediate_bad_ku.key");
    w.csr("intermediate_bad_ku", "/CN=NeoDOS Test Intermediate bad keyUsage/O=NeoDOS Tests/C=ES");
    w.sign("intermediate_bad_ku", "ca_rsa_sha256", "-sha256", VALID_NB, VALID_NA, "ca_bad_ku.ext", false);

    w.genrsa("leaf_rsa_via_bad_ku.key");
    w.csr("leaf_rsa_via_bad_ku", leaf_subject);
    w.sign("leaf_rsa_via_bad_ku", "intermediate_bad_ku", "-sha256", VALID_NB, VALID_NA, "leaf.ext", false);

    // Size boundaries.
    let ca_at_limit = w.padded_ca(8192);
    let ca_over_limit = w.padded_ca(8193);

    Ok(Fixtures {
        ca_rsa_pem: std::fs::read(w.path("ca_rsa_sha256.pem"))?,
        ca_rsa: w.der("ca_rsa_sha256"),
        ca_pss: w.der("ca_rsa_pss"),
        ca_p256: w.der("ca_p256"),
        ca_p384: w.der("ca_p384"),
        ca_no_key_usage: w.der("ca_no_key_usage"),
        ca_bad_key_usage: w.der("ca_bad_key_usage"),
        ca_expired: w.der("ca_expired"),
        ca_not_yet_valid: w.der("ca_not_yet_valid"),
        ca_other_rsa: w.der("ca_other_rsa"),
        ca_at_limit,
        ca_over_limit,
        intermediate_pathlen0: w.der("intermediate_pathlen0"),
        intermediate_pathlen1: w.der("intermediate_pathlen1"),
        intermediate_pathlen300: w.der("intermediate_pathlen300"),
        intermediate_pathlen0_layer: w.der("intermediate_pathlen0_layer"),
        intermediate_pathlen1_layer: w.der("intermediate_pathlen1_layer"),
        intermediate_bad_ku: w.der("intermediate_bad_ku"),
        leaf_rsa: w.der("leaf_rsa"),
        leaf_pss: w.der("leaf_rsa_pss"),
        leaf_p256: w.der("leaf_p256"),
        leaf_p384: w.der("leaf_p384"),
        leaf_wrong_host: w.der("leaf_rsa_wrong_host"),
        leaf_expired: w.der("leaf_rsa_expired"),
        leaf_not_yet: w.der("leaf_rsa_not_yet"),
        leaf_expired_root: w.der("leaf_rsa_expired_root"),
        leaf_valid_expired_root: w.der("leaf_rsa_valid_expired_root"),
        leaf_other_ca: w.der("leaf_rsa_other_ca"),
        leaf_bad_ku: w.der("leaf_rsa_bad_ku"),
        leaf_via_intermediate: w.der("leaf_rsa_via_intermediate"),
        leaf_via_bad_ku: w.der("leaf_rsa_via_bad_ku"),
        leaf_pathlen0: w.der("leaf_rsa_pathlen0"),
        leaf_pathlen300: w.der("leaf_rsa_pathlen300"),
        leaf_pathlen_violation: w.der("leaf_rsa_pathlen_violation"),
    })
}
