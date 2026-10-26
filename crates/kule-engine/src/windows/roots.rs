//! The certificates Windows trusts, as PEM text: what the bundle a task's
//! commands are told to trust begins with, before the proxy's own
//! authority (see `network::trust`).

use std::io;

use windows::Win32::Security::Cryptography::{
    CERT_CONTEXT, CertCloseStore, CertEnumCertificatesInStore, CertOpenSystemStoreW,
};
use windows::core::PCWSTR;

/// Every certificate in the system's `ROOT` store (the machine's and the
/// user's, as Windows shows them together), one PEM block each.
pub(crate) fn pem() -> io::Result<String> {
    use base64::Engine as _;
    let name: Vec<u16> = "ROOT".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the store is closed below; each context points into the
    // store and is read before the next is asked for, which frees it.
    unsafe {
        let store = CertOpenSystemStoreW(None, PCWSTR(name.as_ptr()))
            .map_err(crate::osfs::win::io_error)?;
        let mut text = String::new();
        let mut context: *const CERT_CONTEXT = std::ptr::null();
        loop {
            context = CertEnumCertificatesInStore(store, Some(context));
            if context.is_null() {
                break;
            }
            let cert = &*context;
            let der = std::slice::from_raw_parts(cert.pbCertEncoded, cert.cbCertEncoded as usize);
            let encoded = base64::engine::general_purpose::STANDARD.encode(der);
            text.push_str("-----BEGIN CERTIFICATE-----\n");
            for line in encoded.as_bytes().chunks(64) {
                text.push_str(std::str::from_utf8(line).unwrap_or_default());
                text.push('\n');
            }
            text.push_str("-----END CERTIFICATE-----\n");
        }
        let _ = CertCloseStore(Some(store), 0);
        Ok(text)
    }
}
