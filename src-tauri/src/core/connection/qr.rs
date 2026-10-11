//! Android wireless-debugging **QR** pairing string.
//!
//! The phone's camera reads the string this module produces; the phone answers
//! by starting its pairing server under the requested mDNS instance name, and
//! the host finishes with `adb pair <ip:port> <code>`. Format, exactly as the
//! AOSP docs and Android's own `AdbQrCode` / `AdbDebuggingManager` implement it:
//!
//! ```text
//! WIFI:T:ADB;S:<service name>;P:<pairing code>;;
//! ```
//!
//! `WIFI:` is the ZXing / WPA3 carrier format — no Wi-Fi network is involved:
//! `S` is repurposed as the `_adb-tls-pairing._tcp` instance name the device
//! must publish, and `P` is the shared secret that goes to `adb pair`. The
//! device accepts the code only when `T` is `ADB` and `S`/`P` are non-empty;
//! both values are host-chosen, so they are generated here, escaped the way the
//! format requires, and validated before they are ever shown.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Prefix of the requested service instance name, so a user can tell which host
/// asked for the pairing server (Android Studio uses `studio-`).
const SERVICE_PREFIX: &str = "hermesgate-";
/// Random part of the service name: alphanumeric only, so the name needs no
/// escaping and stays short enough for mDNS (21 characters in total).
const SERVICE_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const SERVICE_RANDOM_LEN: usize = 10;
/// The QR pairing secret is a 10-digit number (AOSP `docs/dev/adb_wifi.md`).
const CODE_LEN: usize = 10;

/// One QR pairing code: the service instance name the phone is asked to publish
/// and the secret it must be paired with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode {
    service: String,
    password: String,
}

impl PairingCode {
    /// A fresh, single-use code. Android requires new credentials for every
    /// attempt: the phone keeps the pairing server only while the flow is up.
    pub fn generate() -> Self {
        Self {
            service: format!(
                "{SERVICE_PREFIX}{}",
                random(SERVICE_ALPHABET, SERVICE_RANDOM_LEN)
            ),
            password: random(b"0123456789", CODE_LEN),
        }
    }

    /// The mDNS instance name the phone must advertise (`_adb-tls-pairing._tcp`).
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The secret handed to `adb pair`.
    pub fn password(&self) -> &str {
        &self.password
    }

    /// The string the QR code carries. The trailing `;;` is the format's empty
    /// final field, not a typo.
    pub fn payload(&self) -> String {
        format!(
            "WIFI:T:ADB;S:{};P:{};;",
            escape(&self.service),
            escape(&self.password)
        )
    }

    /// Read a payload back the way the phone's camera handler does: the scheme
    /// must be `WIFI:`, `T:` must be `ADB`, and `S:`/`P:` must be non-empty.
    /// Unknown fields (`H:`, `I:`, `K:` — the Wi-Fi DPP ones) are ignored.
    pub fn parse(payload: &str) -> Result<Self, String> {
        let payload = payload.trim();
        let body = payload
            .strip_prefix("WIFI:")
            .ok_or_else(|| "not an Android pairing code: it must start with WIFI:".to_string())?;

        let mut security = None;
        let mut service = None;
        let mut password = None;
        for token in split_tokens(body) {
            // The format ends with an empty field (`;;`), and a stray separator
            // is not worth rejecting over.
            if token.is_empty() {
                continue;
            }
            let (key, value) = token
                .split_once(':')
                .ok_or_else(|| format!("malformed pairing field '{token}': expected KEY:VALUE"))?;
            match key {
                "T" => security = Some(unescape(value)),
                "S" => service = Some(unescape(value)),
                "P" => password = Some(unescape(value)),
                _ => {}
            }
        }

        match security.as_deref() {
            Some("ADB") => {}
            Some(other) => return Err(format!("not an ADB pairing code: T:{other}")),
            None => return Err("pairing code has no T:ADB field".to_string()),
        }
        let service = service
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "pairing code has no service name (S:…)".to_string())?;
        let password = password
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "pairing code has no pairing code (P:…)".to_string())?;
        Ok(Self { service, password })
    }
}

/// Encode `payload` as a QR code and return it as the module matrix the UI
/// draws: `(width, modules)` with `modules` row-major and `true` = dark. The
/// quiet zone is the renderer's job.
pub fn modules(payload: &str) -> Result<(usize, Vec<bool>), String> {
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|e| format!("failed to encode the pairing QR code: {e}"))?;
    let width = code.width();
    let modules = code
        .to_colors()
        .into_iter()
        .map(|color| color == qrcode::Color::Dark)
        .collect();
    Ok((width, modules))
}

/// `n` characters from `alphabet`, drawn from OS-seeded SipHash keys.
///
/// ponytail: a one-time pairing secret does not justify a dependency on `rand`
/// — `RandomState` is already keyed from the OS entropy source, the secret is
/// single-use, and it is read off the user's own screen.
fn random(alphabet: &[u8], n: usize) -> String {
    let state = RandomState::new();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let mut out = String::with_capacity(n);
    let mut counter = 0u64;
    while out.len() < n {
        let mut hasher = state.build_hasher();
        hasher.write_u64(counter);
        hasher.write_u128(nanos);
        for byte in hasher.finish().to_le_bytes() {
            if out.len() == n {
                break;
            }
            out.push(alphabet[byte as usize % alphabet.len()] as char);
        }
        counter += 1;
    }
    out
}

/// ZXing escapes `\`, `;`, `,` and `:` with a backslash inside a `WIFI:` payload.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | ';' | ',' | ':') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The inverse of [`escape`]; a trailing lone backslash is kept as it is.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some(escaped) => out.push(escaped),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Split the token list on `;`, leaving escaped separators (`\;`) inside their
/// token for [`unescape`].
fn split_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' => tokens.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    tokens.push(current);
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload in AOSP `docs/dev/adb_wifi.md` — the one Android Studio
    /// generates. Its values contain punctuation, so the parser must not assume
    /// alphanumerics, and neither `@`, `<`, `/` nor `+` may be mangled.
    #[test]
    fn parses_the_payload_android_studio_generates() {
        let code = PairingCode::parse("WIFI:T:ADB;S:studio-g@<xeYnap/;P:(Aq+v9>Cx>!/;;").unwrap();
        assert_eq!(code.service(), "studio-g@<xeYnap/");
        assert_eq!(code.password(), "(Aq+v9>Cx>!/");
    }

    /// Everything the device's own parser (`AdbQrCode`) rejects must be rejected
    /// here too, with a reason, instead of reaching `adb pair`.
    #[test]
    fn rejects_what_the_device_would_reject() {
        for payload in [
            "",
            "WIFI:T:WPA;S:name;P:secret;;",
            "WIFI:S:name;P:secret;;",
            "WIFI:T:ADB;P:secret;;",
            "WIFI:T:ADB;S:name;;",
            "WIFI:T:ADB;S:;P:secret;;",
            "WIFI:T:ADB;S:name;P:;;",
            "DPP:K:public;;",
            "not a pairing code",
        ] {
            assert!(
                PairingCode::parse(payload).is_err(),
                "'{payload}' must not parse"
            );
        }
    }

    /// A generated code has the shape the device expects, and the payload makes
    /// the round trip the UI depends on.
    #[test]
    fn generated_codes_are_well_formed_and_round_trip() {
        let code = PairingCode::generate();
        assert!(code.service().starts_with(SERVICE_PREFIX));
        assert_eq!(
            code.service().len(),
            SERVICE_PREFIX.len() + SERVICE_RANDOM_LEN
        );
        // Only the random part has to be alphanumeric; the prefix carries the
        // separating hyphen, which an mDNS instance name allows.
        assert!(code.service()[SERVICE_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(code.password().len(), CODE_LEN);
        assert!(code.password().chars().all(|c| c.is_ascii_digit()));
        assert_eq!(PairingCode::parse(&code.payload()).unwrap(), code);
    }

    /// Two attempts must never share credentials — the phone keeps the pairing
    /// server only for the ticket it was given.
    #[test]
    fn generated_codes_differ() {
        assert_ne!(PairingCode::generate(), PairingCode::generate());
    }

    /// Separators are escaped on the way out and restored on the way in.
    #[test]
    fn escapes_separators_in_both_directions() {
        let code = PairingCode {
            service: "a;b".to_string(),
            password: "c:d".to_string(),
        };
        assert_eq!(code.payload(), "WIFI:T:ADB;S:a\\;b;P:c\\:d;;");
        assert_eq!(PairingCode::parse(&code.payload()).unwrap(), code);
        assert_eq!(unescape("a\\;b"), "a;b");
        assert_eq!(unescape("a\\"), "a\\");
    }

    /// The matrix is square, complete, and carries the three finder patterns a
    /// camera locks onto before it can read anything.
    #[test]
    fn encodes_a_square_matrix_with_finder_patterns() {
        let payload = PairingCode::generate().payload();
        let (width, modules) = modules(&payload).unwrap();
        assert!((21..=177).contains(&width));
        assert_eq!(modules.len(), width * width);

        let dark = |x: usize, y: usize| modules[y * width + x];
        for (x, y) in [
            (0, 0),
            (6, 0),
            (0, 6),
            (width - 1, 0),
            (width - 7, 0),
            (width - 1, 6),
            (0, width - 1),
            (6, width - 1),
            (0, width - 7),
        ] {
            assert!(dark(x, y), "finder pattern pixel ({x},{y}) must be dark");
        }
        // The eye of the top-left finder is light.
        assert!(!dark(1, 1));
    }
}
