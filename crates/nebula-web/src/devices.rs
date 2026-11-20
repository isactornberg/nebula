//! PAIRED DEVICES: what reaches the page over the tailnet ([`crate::tailnet`])
//! once paired. The Mac's page opens a PAIRING, a one-time code valid for
//! [`PAIRING_TTL`], shown as a link and its QR code; a device that brings
//! the code gets a token in a cookie ([`COOKIE`]) that it sends with every
//! request after. Only a SHA-256 hash of each token is kept, in
//! `devices.json` in nebula's data dir, beside the device's name, the
//! Tailscale user who paired it, and when it was paired and last used. A
//! device is let in only as the user who paired it.
//!
//! A paired device that turns notifications on keeps its push subscription
//! beside it, a file of its own under `push/` in the same dir
//! ([`nebula_forge::push`]), which the DAEMON reads to push to it when a
//! session turns red: removing the device, or pairing it again, drops it.

use crate::api;
use axum::http::{header, HeaderMap};
use nebula_forge::push::{self, Kept, Subscription, Vapid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// The cookie a paired device sends its token in.
pub const COOKIE: &str = "nebula_device";
/// How long a cookie lasts: the most a browser keeps one.
const COOKIE_AGE_SECS: u64 = 400 * 86_400;
/// How long a pairing's code pairs.
const PAIRING_TTL: i64 = 5 * 60_000;
/// How many wrong codes void a pairing.
const WRONG_MAX: u32 = 10;
/// A code's letters: no 0, 1, I or O, which read as one another.
const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 8;
/// A device's last use is written at most this often.
const LAST_USE_EVERY: i64 = 60_000;
/// The longest name a device keeps.
const NAME_MAX: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    id: String,
    name: String,
    login: String,
    /// SHA-256 of its token, in hex.
    hash: String,
    paired_at: i64,
    last_used: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    devices: Vec<Device>,
}

#[derive(Debug, Clone)]
struct Pairing {
    code: String,
    until: i64,
    wrong: u32,
}

/// Why a device is not let in, or a code does not pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// It sent no token, or one no paired device has.
    NotPaired,
    /// It was paired by another Tailscale user.
    OtherUser,
    /// No pairing is open, or its code expired or was used.
    NoPairing,
    /// The code is not the open pairing's.
    WrongCode,
}

impl Refused {
    pub fn message(self) -> &'static str {
        match self {
            Refused::NotPaired => "Pair this device from nebula on your Mac.",
            Refused::OtherUser => "This device was paired by another Tailscale user.",
            Refused::NoPairing => {
                "That code has expired or was used already: make a new one with Pair a phone on your Mac."
            }
            Refused::WrongCode => "That is not the code your Mac shows.",
        }
    }
}

#[derive(Default)]
struct Held {
    devices: Vec<Device>,
    pairing: Option<Pairing>,
}

/// The paired devices and the open pairing, kept in `devices.json`.
#[derive(Clone)]
pub struct Devices {
    held: Arc<Mutex<Held>>,
    file: Arc<PathBuf>,
    /// The dir of their push subscriptions, and the VAPID key file.
    push: Arc<PathBuf>,
    vapid: Arc<PathBuf>,
    /// Bumped whenever a device is removed, so a stream open to it ends.
    removed: Arc<watch::Sender<u64>>,
}

impl Devices {
    /// The devices `file` keeps: none when there is no file or it does
    /// not read. A subscription kept for no device of them goes.
    pub fn load(file: PathBuf) -> Devices {
        let devices: Vec<Device> = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
            .map(|f| f.devices)
            .unwrap_or_default();
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let push = push::push_dir(&dir);
        for kept in push::kept(&push) {
            if !devices.iter().any(|d| d.id == kept.device) {
                let _ = std::fs::remove_file(kept.file);
            }
        }
        Devices {
            held: Arc::new(Mutex::new(Held {
                devices,
                pairing: None,
            })),
            file: Arc::new(file),
            push: Arc::new(push),
            vapid: Arc::new(push::vapid_file(&dir)),
            removed: Arc::new(watch::channel(0).0),
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Open a pairing with a new code, in place of any open one.
    pub fn open_pairing(&self) {
        let code = random(CODE_LEN)
            .into_iter()
            .map(|b| ALPHABET[usize::from(b) % ALPHABET.len()] as char)
            .collect();
        self.held().pairing = Some(Pairing {
            code,
            until: now() + PAIRING_TTL,
            wrong: 0,
        });
    }

    /// Whether `code` pairs now, without using it. A wrong one counts
    /// as [`pair`](Self::pair) counts it, so this is no way round that.
    pub fn pairs(&self, code: &str) -> Result<(), Refused> {
        try_code(&mut self.held(), code, now())
    }

    /// Pair a device named `name` for the Tailscale user `login` with
    /// `code`, once: the token for its cookie. A wrong code counts against
    /// the pairing, which [`WRONG_MAX`] of them void. A device that sends
    /// the token of one paired before is paired in its place.
    pub fn pair(
        &self,
        code: &str,
        name: &str,
        login: &str,
        before: Option<&str>,
    ) -> Result<String, Refused> {
        let now = now();
        let mut held = self.held();
        try_code(&mut held, code, now)?;
        held.pairing = None;
        let token = hex(&random(32));
        if let Some(before) = before.map(hash) {
            for old in held.devices.iter().filter(|d| d.hash == before) {
                push::forget(&self.push, &old.id);
            }
            held.devices.retain(|d| d.hash != before);
        }
        let name: String = name.trim().chars().take(NAME_MAX).collect();
        held.devices.push(Device {
            id: hex(&random(8)),
            name: if name.is_empty() {
                "Device".into()
            } else {
                name
            },
            login: login.to_string(),
            hash: hash(&token),
            paired_at: now,
            last_used: now,
        });
        self.save(&held);
        Ok(token)
    }

    /// The id of the device whose token is `token`, used now by the
    /// Tailscale user `login`.
    pub fn check(&self, token: Option<&str>, login: &str) -> Result<String, Refused> {
        let hash = hash(token.ok_or(Refused::NotPaired)?);
        let now = now();
        let mut held = self.held();
        let device = held
            .devices
            .iter_mut()
            .find(|d| d.hash == hash)
            .ok_or(Refused::NotPaired)?;
        if device.login != login {
            return Err(Refused::OtherUser);
        }
        let id = device.id.clone();
        if now - device.last_used >= LAST_USE_EVERY {
            device.last_used = now;
            self.save(&held);
        }
        Ok(id)
    }

    /// Whether the device `id` is still paired.
    pub fn has(&self, id: &str) -> bool {
        self.held().devices.iter().any(|d| d.id == id)
    }

    /// Remove the device `id`, and its push subscription; whether there was
    /// one.
    pub fn remove(&self, id: &str) -> bool {
        let mut held = self.held();
        push::forget(&self.push, id);
        let before = held.devices.len();
        held.devices.retain(|d| d.id != id);
        if held.devices.len() == before {
            return false;
        }
        self.save(&held);
        drop(held);
        self.removed.send_modify(|n| *n += 1);
        true
    }

    /// Keep `subscription` for the device `id` while it is paired, in place
    /// of any it had: `Err` is why not.
    pub fn subscribe(&self, id: &str, subscription: &Subscription) -> Result<(), String> {
        if let Some(problem) = subscription.problem() {
            return Err(problem.into());
        }
        // Under the lock a removal takes, so no subscription outlives its
        // device.
        let held = self.held();
        if !held.devices.iter().any(|d| d.id == id) {
            return Err(Refused::NotPaired.message().into());
        }
        push::keep(&self.push, id, subscription).map_err(|e| {
            eprintln!("nebula serve: {} not written: {e}", self.push.display());
            "The subscription could not be kept.".to_string()
        })
    }

    /// Drop the device `id`'s push subscription.
    pub fn unsubscribe(&self, id: &str) {
        let _held = self.held();
        push::forget(&self.push, id);
    }

    /// The push subscriptions the device `id` has: one, or none while its
    /// notifications are off.
    pub fn subscriptions(&self, id: &str) -> Vec<Kept> {
        push::kept(&self.push)
            .into_iter()
            .filter(|k| k.device == id)
            .collect()
    }

    /// The device `id`'s name.
    pub fn name(&self, id: &str) -> Option<String> {
        self.held()
            .devices
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
    }

    /// This machine's VAPID key pair, made the first time it is asked for.
    pub fn vapid(&self) -> std::io::Result<Vapid> {
        let _held = self.held();
        Vapid::read_or_make(&self.vapid)
    }

    /// Drop the subscription kept in `kept`, which its push service says is
    /// gone.
    pub fn gone(&self, kept: &Kept) {
        let _ = std::fs::remove_file(&kept.file);
    }

    /// Bumped whenever a device is removed.
    pub fn removals(&self) -> watch::Receiver<u64> {
        self.removed.subscribe()
    }

    /// The devices, newest first, and the open pairing for a machine whose
    /// tailnet name is `tailnet`.
    pub fn list(&self, tailnet: Option<String>) -> api::Devices {
        let mut held = self.held();
        let pairing = match (&tailnet, open(&mut held, now())) {
            (Some(name), Some(p)) => {
                let url = format!("https://{name}/pair/{}", p.code);
                Some(api::Pairing {
                    code: format!("{}-{}", &p.code[..4], &p.code[4..]),
                    qr: qr(&url),
                    url,
                    expires_at: p.until,
                })
            }
            _ => None,
        };
        let pushed = push::kept(&self.push);
        let mut devices: Vec<api::Device> = held
            .devices
            .iter()
            .map(|d| api::Device {
                id: d.id.clone(),
                name: d.name.clone(),
                login: d.login.clone(),
                paired_at: d.paired_at,
                last_used: d.last_used,
                push: pushed.iter().any(|k| k.device == d.id),
            })
            .collect();
        devices.sort_by_key(|d| std::cmp::Reverse(d.paired_at));
        api::Devices {
            tailnet,
            devices,
            pairing,
        }
    }

    /// Write the devices to the file, renamed into place, readable by the
    /// user alone.
    fn save(&self, held: &Held) {
        let file = File {
            devices: held.devices.clone(),
        };
        let written = serde_json::to_vec_pretty(&file)
            .map_err(std::io::Error::other)
            .and_then(|json| push::write_private(&self.file, &json));
        if let Err(err) = written {
            eprintln!("nebula serve: {} not written: {err}", self.file.display());
        }
    }
}

/// The open pairing, once its code has not expired.
fn open(held: &mut Held, now: i64) -> Option<&mut Pairing> {
    if held.pairing.as_ref().is_some_and(|p| p.until <= now) {
        held.pairing = None;
    }
    held.pairing.as_mut()
}

/// Whether `code` is the open pairing's: a wrong one counts against the
/// pairing, which [`WRONG_MAX`] of them void.
fn try_code(held: &mut Held, code: &str, now: i64) -> Result<(), Refused> {
    let pairing = open(held, now).ok_or(Refused::NoPairing)?;
    if pairing.code == normal(code) {
        return Ok(());
    }
    pairing.wrong += 1;
    if pairing.wrong >= WRONG_MAX {
        held.pairing = None;
    }
    Err(Refused::WrongCode)
}

/// A code as typed, as its pairing holds it: upper case, with no spaces or
/// dashes.
fn normal(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub(crate) fn random(n: usize) -> Vec<u8> {
    use rand::Rng;
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// The device token a request's cookie carries.
pub fn token_in(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
        .filter(|token| !token.is_empty())
}

/// The `Set-Cookie` that keeps `token` on a device: sent back with every
/// request to this host over HTTPS, never to a script. `Lax`, not
/// `Strict`, so an app opened from another app (an installed Android app's
/// icon) is sent it; another site's page still cannot act with it, as the
/// guard refuses anything but a read from another origin.
pub fn set_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; Path=/; Max-Age={COOKIE_AGE_SECS}; HttpOnly; Secure; SameSite=Lax")
}

/// A name for the device a browser's `User-Agent` says it is.
pub fn name_from(agent: &str) -> &'static str {
    [
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("Macintosh", "Mac"),
        ("Windows", "Windows PC"),
        ("Linux", "Linux"),
    ]
    .iter()
    .find(|(said, _)| agent.contains(said))
    .map_or("Device", |(_, name)| name)
}

/// `text`'s QR code, a row of `1` (dark) and `0` (light) for each line of
/// modules, with no quiet zone around it.
fn qr(text: &str) -> Vec<String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(code) = QrCode::encode_text(text, QrCodeEcc::Medium) else {
        return Vec::new();
    };
    (0..code.size())
        .map(|y| {
            (0..code.size())
                .map(|x| if code.get_module(x, y) { '1' } else { '0' })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> (tempfile::TempDir, Devices) {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::load(dir.path().join("data/devices.json"));
        (dir, devices)
    }

    fn open_code(devices: &Devices) -> String {
        devices.held().pairing.as_ref().unwrap().code.clone()
    }

    #[test]
    fn a_code_pairs_once_and_only_while_it_is_open() {
        let (_dir, devices) = devices();
        assert_eq!(
            devices.pair("ABCDEFGH", "iPhone", "ada@example.com", None),
            Err(Refused::NoPairing),
            "no pairing open"
        );
        devices.open_pairing();
        let code = open_code(&devices);
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.bytes().all(|b| ALPHABET.contains(&b)), "{code}");
        // As typed: lower case, with the dash the page shows.
        let typed = format!("{}-{}", &code[..4], &code[4..]).to_lowercase();
        assert_eq!(devices.pairs(&typed), Ok(()));
        let token = devices
            .pair(&typed, "iPhone", "ada@example.com", None)
            .unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(
            devices.pair(&code, "iPhone", "ada@example.com", None),
            Err(Refused::NoPairing),
            "used once"
        );
        assert_eq!(devices.pairs(&code), Err(Refused::NoPairing));

        devices.open_pairing();
        let code = open_code(&devices);
        devices.held().pairing.as_mut().unwrap().until = now() - 1;
        assert_eq!(devices.pairs(&code), Err(Refused::NoPairing));
        assert_eq!(
            devices.pair(&code, "iPhone", "ada@example.com", None),
            Err(Refused::NoPairing),
            "expired"
        );
        assert!(devices
            .list(Some("my-mac.tailnet-name.ts.net".into()))
            .pairing
            .is_none());
    }

    #[test]
    fn a_new_pairing_voids_the_last_and_wrong_codes_void_it() {
        let (_dir, devices) = devices();
        devices.open_pairing();
        let first = open_code(&devices);
        devices.open_pairing();
        let right = open_code(&devices);
        assert_ne!(first, right);
        assert_eq!(
            devices.pairs(&first),
            Err(Refused::WrongCode),
            "voided by the new one"
        );
        // A look at a code counts as a try does.
        assert_eq!(devices.pairs("2222-2222"), Err(Refused::WrongCode));
        for _ in 0..WRONG_MAX - 3 {
            assert_eq!(
                devices.pair("2222-2222", "x", "ada@example.com", None),
                Err(Refused::WrongCode)
            );
        }
        assert_eq!(devices.pairs(&right), Ok(()), "still open");
        assert_eq!(
            devices.pair("2222-2222", "x", "ada@example.com", None),
            Err(Refused::WrongCode)
        );
        assert_eq!(
            devices.pair(&right, "x", "ada@example.com", None),
            Err(Refused::NoPairing),
            "voided by the wrong ones"
        );
    }

    #[test]
    fn only_a_hash_of_the_token_is_on_disk_and_only_for_its_user() {
        let (dir, devices) = devices();
        devices.open_pairing();
        let token = devices
            .pair(&open_code(&devices), "  iPhone  ", "ada@example.com", None)
            .unwrap();
        let file = dir.path().join("data/devices.json");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains(&token), "{text}");
        assert!(text.contains(&hash(&token)), "{text}");
        assert!(text.contains(r#""login": "ada@example.com""#), "{text}");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let id = devices.check(Some(&token), "ada@example.com").unwrap();
        assert_eq!(
            devices.check(Some(&token), "eve@example.com"),
            Err(Refused::OtherUser)
        );
        assert_eq!(
            devices.check(Some("0".repeat(64).as_str()), "ada@example.com"),
            Err(Refused::NotPaired)
        );
        assert_eq!(
            devices.check(None, "ada@example.com"),
            Err(Refused::NotPaired)
        );

        // A server that starts again knows it.
        let again = Devices::load(file.clone());
        assert_eq!(again.check(Some(&token), "ada@example.com"), Ok(id.clone()));
        let listed = again.list(None).devices;
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].name.as_str(), listed[0].login.as_str()),
            ("iPhone", "ada@example.com")
        );

        let removals = devices.removals();
        assert!(devices.remove(&id));
        assert!(removals.has_changed().unwrap());
        assert!(!devices.remove(&id));
        assert_eq!(
            devices.check(Some(&token), "ada@example.com"),
            Err(Refused::NotPaired)
        );
        assert!(Devices::load(file).list(None).devices.is_empty());
    }

    #[test]
    fn pairing_again_replaces_the_device_it_was() {
        let (_dir, devices) = devices();
        devices.open_pairing();
        let first = devices
            .pair(&open_code(&devices), "iPhone", "ada@example.com", None)
            .unwrap();
        devices.open_pairing();
        let second = devices
            .pair(
                &open_code(&devices),
                "iPhone",
                "ada@example.com",
                Some(&first),
            )
            .unwrap();
        assert_eq!(devices.list(None).devices.len(), 1);
        assert_eq!(
            devices.check(Some(&first), "ada@example.com"),
            Err(Refused::NotPaired)
        );
        assert!(devices.check(Some(&second), "ada@example.com").is_ok());
    }

    fn subscription(endpoint: &str) -> Subscription {
        Subscription {
            endpoint: endpoint.into(),
            p256dh: "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4".into(),
            auth: "BTBZMqHH6r4Tts7J_aSIgg".into(),
        }
    }

    /// A subscription lives as long as its device: one kept for a device
    /// paired again with its old cookie goes, as does one a server finds
    /// on disk for no device it keeps, and none is kept for a device that
    /// is not paired.
    #[test]
    fn a_subscription_goes_with_its_device() {
        let (dir, devices) = devices();
        devices.open_pairing();
        let first = devices
            .pair(&open_code(&devices), "iPhone", "ada@example.com", None)
            .unwrap();
        let id = devices.check(Some(&first), "ada@example.com").unwrap();
        let sub = subscription("https://web.push.apple.com/QGuQyavXut");
        devices.subscribe(&id, &sub).unwrap();
        assert!(devices.list(None).devices[0].push);
        assert_eq!(
            devices.subscribe("0000000000000000", &sub),
            Err(Refused::NotPaired.message().to_string())
        );
        assert!(devices
            .subscribe(&id, &subscription("http://web.push.apple.com/x"))
            .is_err());

        devices.open_pairing();
        let second = devices
            .pair(
                &open_code(&devices),
                "iPhone",
                "ada@example.com",
                Some(&first),
            )
            .unwrap();
        let new_id = devices.check(Some(&second), "ada@example.com").unwrap();
        assert!(devices.subscriptions(&id).is_empty(), "paired again");
        assert!(!devices.list(None).devices[0].push);

        devices.subscribe(&new_id, &sub).unwrap();
        let push = dir.path().join("data/push");
        push::keep(&push, "ffffffffffffffff", &sub).unwrap();
        assert_eq!(push::kept(&push).len(), 2);
        let again = Devices::load(dir.path().join("data/devices.json"));
        let kept = push::kept(&push);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].device, new_id);
        assert_eq!(again.subscriptions(&new_id).len(), 1);
    }

    #[test]
    fn the_pairing_is_listed_as_a_link_a_code_and_a_qr_code() {
        let (_dir, devices) = devices();
        let name = Some("my-mac.tailnet-name.ts.net".to_string());
        assert!(devices.list(name.clone()).pairing.is_none());
        devices.open_pairing();
        assert!(
            devices.list(None).pairing.is_none(),
            "no tailnet name to link to"
        );
        let code = open_code(&devices);
        let pairing = devices.list(name).pairing.unwrap();
        assert_eq!(
            pairing.url,
            format!("https://my-mac.tailnet-name.ts.net/pair/{code}")
        );
        assert_eq!(pairing.code, format!("{}-{}", &code[..4], &code[4..]));
        let size = pairing.qr.len();
        assert!(
            size >= 21 && (size - 21) % 4 == 0,
            "a QR code's size: {size}"
        );
        assert!(pairing.qr.iter().all(|row| row.len() == size));
        // A finder pattern's corner.
        assert!(pairing.qr[0].starts_with("1111111"));
    }

    #[test]
    fn the_token_is_read_from_its_cookie_alone() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            "a=1; nebula_device=abc; b=2".parse().unwrap(),
        );
        assert_eq!(token_in(&headers), Some("abc"));
        let mut other = HeaderMap::new();
        other.append(
            header::COOKIE,
            "nebula_device_old=abc; nebula_device=".parse().unwrap(),
        );
        assert_eq!(token_in(&other), None);
        let cookie = set_cookie("abc");
        for part in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/", "Max-Age="] {
            assert!(cookie.contains(part), "{cookie}");
        }
    }

    #[test]
    fn a_device_is_named_by_its_browser() {
        let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1";
        assert_eq!(name_from(iphone), "iPhone");
        assert_eq!(
            name_from("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"),
            "Mac"
        );
        assert_eq!(name_from("curl/8.7.1"), "Device");
    }
}
