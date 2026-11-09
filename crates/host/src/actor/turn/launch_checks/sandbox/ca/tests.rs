//! The jail's CA bundle takes only the certificate blocks of the agent's `network.ca_certs`
//! file: a private key in the same file never lands in the jail.

use std::sync::Arc;

use super::{CA_FILE, certificates, write};
use crate::config::sandbox::CaRoots;

const CERT: &str = "-----BEGIN CERTIFICATE-----\nMIIBfirm\n-----END CERTIFICATE-----";
const KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEsecret\n-----END PRIVATE KEY-----";

#[test]
fn a_ca_file_with_a_key_and_a_certificate_gives_the_bundle_only_the_certificate() {
    let root = std::env::temp_dir().join(format!("clauseo-d2a-ca-{}", std::process::id()));
    crate::write::create_dir_all(&root).unwrap();
    let file = root.join("firm.pem");
    let text = format!("subject=firm\n{KEY}\n{CERT}\n# trailing words\n");
    crate::write::write_file(&file, text.as_bytes()).unwrap();
    let roots: CaRoots = Arc::new(Vec::new);
    let bundle = write(&root, Some(&roots), Some(&file)).unwrap();
    assert_eq!(bundle, root.join(CA_FILE));
    let pem = std::fs::read_to_string(&bundle).unwrap();
    assert_eq!(pem, format!("{CERT}\n"));
    assert!(!pem.contains("PRIVATE KEY") && !pem.contains("secret"));
    // A file with a key and no certificate adds nothing.
    crate::write::write_file(&file, KEY.as_bytes()).unwrap();
    let pem = std::fs::read_to_string(write(&root, Some(&roots), Some(&file)).unwrap()).unwrap();
    assert_eq!(pem, "");
    crate::write::remove_dir_all(&root).unwrap();
}

#[test]
fn only_whole_certificate_blocks_are_taken() {
    let two = format!("{CERT}\n{KEY}\n{CERT}\n");
    assert_eq!(certificates(&two), [CERT, CERT]);
    // A BEGIN with no END of its own, then a whole block: only the whole block.
    let broken = format!("-----BEGIN CERTIFICATE-----\nMIIBcut\n{CERT}");
    assert_eq!(certificates(&broken), [CERT]);
    assert_eq!(
        certificates("-----BEGIN CERTIFICATE-----\nMIIBcut"),
        Vec::<String>::new()
    );
    assert_eq!(certificates(KEY), Vec::<String>::new());
}
