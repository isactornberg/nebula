//! WEB PUSH: a notification sent from this machine straight to a paired
//! phone's push service (Apple's, Google's, Mozilla's), with no relay of
//! nebula's own. The message is encrypted for the phone alone, as RFC 8291
//! says (`aes128gcm`), and signed as from this machine with its VAPID key,
//! as RFC 8292 says (an ES256 JWT), so the push service takes messages for
//! the phone from no one else.
//!
//! What it keeps is in nebula's data dir: the VAPID key pair in
//! `vapid.json`, made once, and each paired device's subscription in a
//! file of its own under `push/`, named by the device and its endpoint, so
//! `nebula serve`, which writes them, and the DAEMON, which drops one the
//! push service says is gone, never write the same file. Each is written
//! renamed into place, readable by the user alone.
//!
//! A message goes out through `curl`, as Linear's calls do ([`crate::linear`]):
//! the endpoint, which lets anyone holding it push to the phone, and the
//! JWT reach it on stdin as a config, never in argv; the encrypted body,
//! which says nothing to anyone without the phone's key, from a file.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::Aes128Gcm;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::Generate;
use p256::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Who the JWT says sends: a contact the push service may write to, which
/// RFC 8292 asks for. Generic on purpose: push services are outside
/// services, and nothing of the user's goes to them.
pub const SUBJECT: &str = "mailto:push@nebula.invalid";
/// How long a JWT is good for: under the day RFC 8292 allows.
const JWT_SECS: u64 = 12 * 3600;
/// How long the push service keeps a message for a phone that is offline.
const TTL_SECS: u32 = 3600;
/// The one record a message is: RFC 8291's record size.
const RECORD_SIZE: u32 = 4096;
/// The longest title and body a message carries, in characters, which
/// keeps it well inside the 4 KiB a push service takes.
const TITLE_MAX: usize = 120;
const BODY_MAX: usize = 400;
/// The longest endpoint a subscription is taken with.
const ENDPOINT_MAX: usize = 2048;
/// The budget a send gets: past it the `curl` is killed.
const TIMEOUT: std::time::Duration = crate::pull_request::TIMEOUT;

/// Where nebula keeps its VAPID key pair.
pub fn vapid_file(data: &Path) -> PathBuf {
    data.join("vapid.json")
}

/// Where nebula keeps the paired devices' subscriptions.
pub fn push_dir(data: &Path) -> PathBuf {
    data.join("push")
}

/// This machine's VAPID key pair: what a phone subscribes with, and what
/// every message to it is signed with.
pub struct Vapid(SecretKey);

#[derive(Serialize, Deserialize)]
struct VapidFile {
    /// The private key's scalar, base64url.
    private: String,
}

impl Vapid {
    /// The key pair `file` keeps, if it keeps one that reads.
    pub fn read(file: &Path) -> Option<Vapid> {
        let kept: VapidFile = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        let bytes = B64.decode(kept.private).ok()?;
        SecretKey::from_slice(&bytes).ok().map(Vapid)
    }

    /// The key pair `file` keeps, made and kept there first when it keeps
    /// none. A file that does not read is replaced: no subscription made
    /// with the key it held can be sent to without that key anyway.
    pub fn read_or_make(file: &Path) -> std::io::Result<Vapid> {
        if let Some(vapid) = Vapid::read(file) {
            return Ok(vapid);
        }
        let secret = SecretKey::generate_from_rng(&mut rand::rng());
        let kept = VapidFile {
            private: B64.encode(secret.to_bytes()),
        };
        write_private(file, &serde_json::to_vec_pretty(&kept)?)?;
        Ok(Vapid(secret))
    }

    /// The public key, base64url, uncompressed: a page's
    /// `applicationServerKey`, and the `k` of each message.
    pub fn public(&self) -> String {
        B64.encode(self.0.public_key().to_sec1_bytes())
    }

    /// The `Authorization` header for a message to `endpoint`, signed for
    /// its origin until `now_secs` + [`JWT_SECS`].
    fn authorization(&self, endpoint: &str, now_secs: u64) -> String {
        format!(
            "vapid t={}, k={}",
            self.jwt(endpoint, now_secs),
            self.public()
        )
    }

    fn jwt(&self, endpoint: &str, now_secs: u64) -> String {
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({
            "aud": origin(endpoint),
            "exp": now_secs + JWT_SECS,
            "sub": SUBJECT,
        });
        let input = format!("{header}.{}", B64.encode(claims.to_string()));
        let signature: Signature = SigningKey::from(&self.0).sign(input.as_bytes());
        format!("{input}.{}", B64.encode(signature.to_bytes()))
    }
}

/// `https://host[:port]` of an endpoint: the audience its JWT names.
fn origin(endpoint: &str) -> &str {
    let rest = endpoint.split_once("://").map_or(endpoint, |(_, r)| r);
    let host_len = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    &endpoint[..endpoint.len() - rest.len() + host_len]
}

/// A browser's push subscription, as its `toJSON()` has it: where its push
/// service takes messages for it, and the keys it decrypts them with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    /// Its P-256 public key, uncompressed, base64url.
    pub p256dh: String,
    /// Its 16-byte authentication secret, base64url.
    pub auth: String,
}

impl Subscription {
    /// Why this is no subscription a message could be sent to, if it is
    /// not: an endpoint that is not an `https://` URL of a host, or keys
    /// that are not a P-256 point and 16 bytes.
    pub fn problem(&self) -> Option<&'static str> {
        let host = self
            .endpoint
            .strip_prefix("https://")
            .map(|rest| &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())]);
        let host_ok = host.is_some_and(|h| {
            !h.is_empty() && !h.contains('@') && h.chars().all(|c| c.is_ascii_graphic())
        });
        if !host_ok
            || self.endpoint.len() > ENDPOINT_MAX
            || self
                .endpoint
                .chars()
                .any(|c| c.is_control() || c == '"' || c == '\\')
        {
            return Some("A subscription's endpoint must be an https:// URL of its push service.");
        }
        if self.keys().is_none() {
            return Some("A subscription's keys must be a P-256 public key and a 16-byte secret.");
        }
        None
    }

    fn keys(&self) -> Option<(PublicKey, [u8; 16])> {
        let public = PublicKey::from_sec1_bytes(&B64.decode(&self.p256dh).ok()?).ok()?;
        let auth = B64.decode(&self.auth).ok()?.try_into().ok()?;
        Some((public, auth))
    }
}

/// A subscription kept for a paired device, and the file it is kept in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub device: String,
    #[serde(flatten)]
    pub subscription: Subscription,
    #[serde(skip)]
    pub file: PathBuf,
}

/// Every subscription kept in `dir`.
pub fn kept(dir: &Path) -> Vec<Kept> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut all: Vec<Kept> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|file| {
            let mut kept: Kept = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            kept.file = file;
            Some(kept)
        })
        .collect();
    all.sort_by(|a, b| a.file.cmp(&b.file));
    all
}

/// Keep `subscription` for `device` in `dir`, in place of any it had.
pub fn keep(dir: &Path, device: &str, subscription: &Subscription) -> std::io::Result<()> {
    let hash = Sha256::digest(subscription.endpoint.as_bytes());
    let name: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    let file = dir.join(format!("{device}-{name}.json"));
    let kept = Kept {
        device: device.to_string(),
        subscription: subscription.clone(),
        file: file.clone(),
    };
    write_private(&file, &serde_json::to_vec_pretty(&kept)?)?;
    for other in kept_for(dir, device).filter(|k| k.file != file) {
        let _ = std::fs::remove_file(other.file);
    }
    Ok(())
}

/// Drop every subscription `device` has in `dir`.
pub fn forget(dir: &Path, device: &str) {
    for kept in kept_for(dir, device) {
        let _ = std::fs::remove_file(kept.file);
    }
}

fn kept_for<'a>(dir: &Path, device: &'a str) -> impl Iterator<Item = Kept> + 'a {
    kept(dir).into_iter().filter(move |k| k.device == device)
}

/// Write `bytes` to `path`, renamed into place so no reader sees half of
/// it, readable by the user alone, in a folder only the user may list.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes))
        .and_then(|()| std::fs::rename(&tmp, path))
}

/// What a notification says, and where tapping it goes: the JSON the
/// page's service worker shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// A newer notice with the same tag takes the place of an older one.
    pub tag: String,
    /// The page's path to open.
    pub url: String,
}

impl Notice {
    fn payload(&self) -> Vec<u8> {
        let cut = |s: &str, max: usize| -> String {
            match s.char_indices().nth(max) {
                Some((at, _)) => format!("{}…", s[..at].trim_end()),
                None => s.to_string(),
            }
        };
        let notice = Notice {
            title: cut(&self.title, TITLE_MAX),
            body: cut(&self.body, BODY_MAX),
            ..self.clone()
        };
        serde_json::to_vec(&notice).unwrap_or_default()
    }
}

/// What came of a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Delivered,
    /// The push service says the subscription is gone (404 or 410): the
    /// phone unsubscribed, or its browser dropped it.
    Gone,
    /// Why it was not sent: what the push service answered, or what went
    /// wrong before it could.
    Failed(String),
}

/// Send `notice` to the phone subscribed as `to`, signed with `vapid`.
pub async fn send(vapid: &Vapid, to: &Subscription, notice: &Notice) -> Sent {
    if let Some(problem) = to.problem() {
        return Sent::Failed(problem.into());
    }
    deliver(vapid, to, &notice.payload(), "=https").await
}

/// [`send`] of `payload` by the protocols `protos` lets `curl` use: only
/// HTTPS but in a test of this module.
async fn deliver(vapid: &Vapid, to: &Subscription, payload: &[u8], protos: &str) -> Sent {
    let Some((public, auth)) = to.keys() else {
        return Sent::Failed("the subscription's keys do not read".into());
    };
    let mut salt = [0u8; 16];
    rand::Rng::fill_bytes(&mut rand::rng(), &mut salt);
    let sender = SecretKey::generate_from_rng(&mut rand::rng());
    let body = encrypt(payload, &public, &auth, &sender, &salt);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let config = curl_config(
        &to.endpoint,
        &vapid.authorization(&to.endpoint, now),
        protos,
    );
    match post(&config, &body).await {
        Ok((200..=299, _)) => Sent::Delivered,
        Ok((404 | 410, _)) => Sent::Gone,
        Ok((code, said)) => {
            let said: String = said.split_whitespace().collect::<Vec<_>>().join(" ");
            let said: String = said.chars().take(200).collect();
            Sent::Failed(if said.is_empty() {
                format!("the push service answered {code}")
            } else {
                format!("the push service answered {code}: {said}")
            })
        }
        Err(why) => Sent::Failed(why),
    }
}

/// RFC 8291's `aes128gcm` message of `plain` for the subscriber whose keys
/// are `ua_public` and `auth`, from the sender key `sender` with `salt`:
/// one record, its header carrying the salt, the record size and the
/// sender's public key.
fn encrypt(
    plain: &[u8],
    ua_public: &PublicKey,
    auth: &[u8; 16],
    sender: &SecretKey,
    salt: &[u8; 16],
) -> Vec<u8> {
    let ua = ua_public.to_sec1_bytes();
    let us = sender.public_key().to_sec1_bytes();
    let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), ua_public.as_affine());
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(&ua);
    key_info.extend_from_slice(&us);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .expect("32 bytes is a valid HKDF-SHA-256 length");
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .and_then(|()| prk.expand(b"Content-Encoding: nonce\0", &mut nonce))
        .expect("16 and 12 bytes are valid HKDF-SHA-256 lengths");
    // The last record's padding delimiter, with no padding after it.
    let mut record = plain.to_vec();
    record.push(2);
    let sealed = Aes128Gcm::new(&cek.into())
        .encrypt(&nonce.into(), record.as_slice())
        .expect("AES-GCM seals a record this short");
    let mut out = Vec::with_capacity(16 + 4 + 1 + us.len() + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(us.len() as u8);
    out.extend_from_slice(&us);
    out.extend_from_slice(&sealed);
    out
}

/// The `curl` config fed on stdin: the endpoint and the headers, each a
/// double-quoted value with `\` and `"` escaped, curl's quoting.
fn curl_config(endpoint: &str, authorization: &str, protos: &str) -> String {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut config = format!(
        "url = \"{}\"\nproto = \"{protos}\"\nrequest = \"POST\"\n",
        quote(endpoint)
    );
    for header in [
        format!("Authorization: {authorization}"),
        "Content-Encoding: aes128gcm".into(),
        "Content-Type: application/octet-stream".into(),
        format!("TTL: {TTL_SECS}"),
        "Urgency: high".into(),
    ] {
        config.push_str(&format!("header = \"{}\"\n", quote(&header)));
    }
    config.push_str("write-out = \"\\n%{http_code}\"\n");
    config
}

/// POST `body` by the `curl` config `config`: the status the push service
/// answered and what it said, or why it could not be asked.
async fn post(config: &str, body: &[u8]) -> Result<(u16, String), String> {
    use tokio::io::AsyncWriteExt;
    let file = body_file(body).map_err(|e| format!("couldn't write the message: {e}"))?;
    let quoted = file
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let config = format!("{config}data-binary = \"@{quoted}\"\n");
    let max_time = TIMEOUT.as_secs().to_string();
    let run = async {
        let mut child = tokio::process::Command::new("curl")
            .args(["-sS", "--max-time", &max_time, "-K", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("couldn't run curl: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("couldn't feed curl")?;
        let feed = async move {
            stdin.write_all(config.as_bytes()).await?;
            stdin.shutdown().await
        };
        let (_, out) = tokio::join!(feed, child.wait_with_output());
        match out {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout).into_owned();
                let (said, code) = text.rsplit_once('\n').unwrap_or(("", &text));
                let code = code.trim().parse().map_err(|_| "curl said no status")?;
                Ok((code, said.to_string()))
            }
            Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            Err(e) => Err(format!("curl failed: {e}")),
        }
    };
    let got = tokio::time::timeout(TIMEOUT, run)
        .await
        .unwrap_or_else(|_| Err("the push service timed out".into()));
    let _ = std::fs::remove_file(&file);
    got
}

/// A file holding `body`, readable by the user alone, for `curl` to send.
fn body_file(body: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut name = [0u8; 8];
    rand::Rng::fill_bytes(&mut rand::rng(), &mut name);
    let name: String = name.iter().map(|b| format!("{b:02x}")).collect();
    let file = std::env::temp_dir().join(format!("nebula-push-{name}"));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)?
        .write_all(body)?;
    Ok(file)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aes_gcm::aead::Aead;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn b64(s: &str) -> Vec<u8> {
        B64.decode(s.replace([' ', '\n'], "")).unwrap()
    }

    /// RFC 8291's Appendix A: its keys, salt and plaintext give its message
    /// to the byte.
    #[test]
    fn rfc_8291_appendix_a() {
        let plain = b64("V2hlbiBJIGdyb3cgdXAsIEkgd2FudCB0byBiZSBhIHdhdGVybWVsb24");
        assert_eq!(plain, b"When I grow up, I want to be a watermelon");
        let sender =
            SecretKey::from_slice(&b64("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        assert_eq!(
            sender.public_key().to_sec1_bytes().to_vec(),
            b64("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8")
        );
        let ua = PublicKey::from_sec1_bytes(&b64(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        ))
        .unwrap();
        let auth: [u8; 16] = b64("BTBZMqHH6r4Tts7J_aSIgg").try_into().unwrap();
        let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let message = encrypt(&plain, &ua, &auth, &sender, &salt);
        assert_eq!(
            B64.encode(&message),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
        let ua_private = b64("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94");
        assert_eq!(decrypt(&message, &ua_private, &auth), plain);
    }

    /// The receiving side of RFC 8291, as a phone's browser decrypts a
    /// message: what a stand-in push service checks a sent one with.
    pub(crate) fn decrypt(message: &[u8], ua_private: &[u8], auth: &[u8]) -> Vec<u8> {
        let (salt, rest) = message.split_at(16);
        let (rs, rest) = rest.split_at(4);
        assert_eq!(u32::from_be_bytes(rs.try_into().unwrap()), RECORD_SIZE);
        let (id_len, rest) = rest.split_at(1);
        let (sender, sealed) = rest.split_at(id_len[0] as usize);
        let ua = SecretKey::from_slice(ua_private).unwrap();
        let sender = PublicKey::from_sec1_bytes(sender).unwrap();
        let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), sender.as_affine());
        let mut key_info = b"WebPush: info\0".to_vec();
        key_info.extend_from_slice(&ua.public_key().to_sec1_bytes());
        key_info.extend_from_slice(&sender.to_sec1_bytes());
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
            .expand(&key_info, &mut ikm)
            .unwrap();
        let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
            .unwrap();
        prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
            .unwrap();
        let mut record = Aes128Gcm::new(&cek.into())
            .decrypt(&nonce.into(), sealed)
            .unwrap();
        assert_eq!(record.pop(), Some(2), "the last record's delimiter");
        record
    }

    /// The JWT is ES256 over its header and claims, by the key its `k`
    /// names, for the endpoint's origin, from no one in particular.
    #[test]
    fn the_jwt_is_signed_for_the_endpoint_s_origin_by_the_key_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let vapid = Vapid::read_or_make(&vapid_file(dir.path())).unwrap();
        let endpoint = "https://web.push.apple.com:443/QGuQyavXut?x=1";
        assert_eq!(origin(endpoint), "https://web.push.apple.com:443");
        assert_eq!(
            origin("https://fcm.googleapis.com/fcm/send/x"),
            "https://fcm.googleapis.com"
        );
        let auth = vapid.authorization(endpoint, 1_000);
        let (jwt, k) = auth
            .strip_prefix("vapid t=")
            .and_then(|a| a.split_once(", k="))
            .unwrap();
        assert_eq!(k, vapid.public());
        let claims = check_jwt(jwt, k);
        assert_eq!(
            claims,
            serde_json::json!({"aud": "https://web.push.apple.com:443", "exp": 1_000 + JWT_SECS, "sub": SUBJECT})
        );
        assert!(!SUBJECT.contains('@') || SUBJECT.ends_with("@nebula.invalid"));
    }

    /// `jwt`'s claims, once its ES256 signature checks out against the
    /// public key `k`.
    pub(crate) fn check_jwt(jwt: &str, k: &str) -> serde_json::Value {
        let (input, signature) = jwt.rsplit_once('.').unwrap();
        let (header, claims) = input.split_once('.').unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&b64(header)).unwrap(),
            serde_json::json!({"typ": "JWT", "alg": "ES256"})
        );
        let key = VerifyingKey::from_sec1_bytes(&b64(k)).unwrap();
        let signature = Signature::from_slice(&b64(signature)).unwrap();
        key.verify(input.as_bytes(), &signature).unwrap();
        serde_json::from_slice(&b64(claims)).unwrap()
    }

    #[test]
    fn the_key_pair_is_made_once_and_kept_for_the_user_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = vapid_file(&dir.path().join("data"));
        assert!(Vapid::read(&file).is_none());
        let made = Vapid::read_or_make(&file).unwrap();
        assert_eq!(b64(&made.public()).len(), 65);
        assert_eq!(Vapid::read_or_make(&file).unwrap().public(), made.public());
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(file.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700);
    }

    /// A phone's keys, as a browser makes them, and its subscription to the
    /// push service at `endpoint`.
    pub(crate) fn phone(endpoint: &str) -> (Subscription, Vec<u8>, Vec<u8>) {
        let key = SecretKey::generate_from_rng(&mut rand::rng());
        let mut auth = vec![0u8; 16];
        rand::Rng::fill_bytes(&mut rand::rng(), &mut auth);
        let subscription = Subscription {
            endpoint: endpoint.into(),
            p256dh: B64.encode(key.public_key().to_sec1_bytes()),
            auth: B64.encode(&auth),
        };
        (subscription, key.to_bytes().to_vec(), auth)
    }

    #[test]
    fn only_an_https_endpoint_and_real_keys_are_a_subscription() {
        let (good, _, _) = phone("https://fcm.googleapis.com/fcm/send/abc:def");
        assert_eq!(good.problem(), None);
        for endpoint in [
            "http://fcm.googleapis.com/x",
            "file:///etc/passwd",
            "https://",
            "https://user@evil.example/x",
            "https://a b/x",
            "https://x/\"\nurl = \"https://evil.example",
        ] {
            let bad = Subscription {
                endpoint: endpoint.into(),
                ..good.clone()
            };
            assert!(bad.problem().is_some(), "{endpoint}");
        }
        let long = Subscription {
            endpoint: format!("https://x/{}", "a".repeat(ENDPOINT_MAX)),
            ..good.clone()
        };
        assert!(long.problem().is_some());
        for (p256dh, auth) in [
            (B64.encode([4u8; 65]), good.auth.clone()),
            (good.p256dh.clone(), B64.encode([1u8; 15])),
            ("not base64!".into(), good.auth.clone()),
        ] {
            let bad = Subscription {
                p256dh,
                auth,
                ..good.clone()
            };
            assert!(bad.problem().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn a_device_keeps_one_subscription_in_a_file_of_its_own() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let push = push_dir(dir.path());
        let (first, _, _) = phone("https://fcm.googleapis.com/fcm/send/one");
        let (second, _, _) = phone("https://fcm.googleapis.com/fcm/send/two");
        let (other, _, _) = phone("https://web.push.apple.com/three");
        keep(&push, "aaaa", &first).unwrap();
        keep(&push, "bbbb", &other).unwrap();
        keep(&push, "aaaa", &second).unwrap();
        let all = kept(&push);
        assert_eq!(all.len(), 2, "{all:?}");
        let mine: Vec<_> = all.iter().filter(|k| k.device == "aaaa").collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].subscription, second);
        let mode = std::fs::metadata(&mine[0].file)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        forget(&push, "aaaa");
        let left = kept(&push);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].device, "bbbb");
        assert!(kept(&dir.path().join("nothing")).is_empty());
    }

    #[test]
    fn a_long_notice_is_cut_to_what_a_push_service_takes() {
        let notice = Notice {
            title: "t".repeat(500),
            body: "é".repeat(5000),
            tag: "session-01J".into(),
            url: "/s/01J".into(),
        };
        let payload = notice.payload();
        assert!(payload.len() + 86 + 17 < 4096, "{}", payload.len());
        let back: Notice = serde_json::from_slice(&payload).unwrap();
        assert_eq!(back.title.chars().count(), TITLE_MAX + 1);
        assert!(back.body.ends_with('…'));
        assert_eq!(back.url, "/s/01J");
    }

    /// One HTTP request a stand-in push service took: its head and body.
    struct Took {
        head: String,
        body: Vec<u8>,
    }

    /// A stand-in push service on 127.0.0.1 that answers one request with
    /// `status`, and hands over what it took.
    async fn push_service(status: u16) -> (String, tokio::sync::oneshot::Receiver<Took>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = format!("http://{}/push/abc", listener.local_addr().unwrap());
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            let (head, length) = loop {
                let n = s.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
                if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&got[..end]).to_string();
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    got.drain(..end + 4);
                    break (head, length);
                }
            };
            while got.len() < length {
                let n = s.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Length: 14\r\nConnection: close\r\n\r\n{{\"reason\":\"x\"}}");
            s.write_all(reply.as_bytes()).await.unwrap();
            let _ = tx.send(Took { head, body: got });
        });
        (at, rx)
    }

    fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    /// A send is one POST through `curl`: signed for the push service's
    /// origin, encrypted for the phone alone, and read by its status.
    #[tokio::test]
    async fn a_send_posts_the_message_the_phone_alone_can_read() {
        let dir = tempfile::tempdir().unwrap();
        let vapid = Vapid::read_or_make(&vapid_file(dir.path())).unwrap();
        let (at, took) = push_service(201).await;
        let (to, ua_private, auth) = phone(&at);
        let notice = Notice {
            title: "shopfront › Rotate the staging API key".into(),
            body: "QUESTION: which key?".into(),
            tag: "session-01J".into(),
            url: "/s/01J".into(),
        };
        // A send takes only https: the stand-in speaks plain HTTP.
        assert!(matches!(send(&vapid, &to, &notice).await, Sent::Failed(_)));
        assert_eq!(
            deliver(&vapid, &to, &notice.payload(), "=http").await,
            Sent::Delivered
        );
        let took = took.await.unwrap();
        assert!(
            took.head.starts_with("POST /push/abc HTTP/1.1"),
            "{}",
            took.head
        );
        assert_eq!(header(&took.head, "content-encoding"), Some("aes128gcm"));
        assert_eq!(header(&took.head, "ttl"), Some("3600"));
        assert_eq!(header(&took.head, "urgency"), Some("high"));
        let (jwt, k) = header(&took.head, "authorization")
            .and_then(|a| a.strip_prefix("vapid t="))
            .and_then(|a| a.split_once(", k="))
            .unwrap();
        assert_eq!(k, vapid.public());
        assert_eq!(check_jwt(jwt, k)["aud"], origin(&at));
        let read: Notice =
            serde_json::from_slice(&decrypt(&took.body, &ua_private, &auth)).unwrap();
        assert_eq!(read, notice);
    }

    /// 404 and 410 say the subscription is gone; any other refusal says
    /// what the push service said.
    #[tokio::test]
    async fn a_gone_subscription_is_told_from_a_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let vapid = Vapid::read_or_make(&vapid_file(dir.path())).unwrap();
        for (status, want) in [
            (201, Sent::Delivered),
            (410, Sent::Gone),
            (404, Sent::Gone),
            (
                403,
                Sent::Failed(r#"the push service answered 403: {"reason":"x"}"#.into()),
            ),
        ] {
            let (at, _took) = push_service(status).await;
            let (to, _, _) = phone(&at);
            assert_eq!(deliver(&vapid, &to, b"{}", "=http").await, want, "{status}");
        }
    }

    /// `curl` refuses any protocol but the one it is given, so an endpoint
    /// that names another is never asked.
    #[tokio::test]
    async fn curl_takes_https_alone() {
        let (at, _took) = push_service(201).await;
        let config = curl_config(&at, "vapid t=x, k=y", "=https");
        let got = post(&config, b"x").await;
        assert!(got.is_err(), "{got:?}");
    }
}
