//! TOTP (RFC 6238) for the access gate: 6 digits, 30 s steps, SHA-1, the
//! defaults every authenticator app (and iOS Passwords) understands.
//!
//! A 6-digit code is far easier to guess than a long password, so on top of
//! the gate's per-IP lockout:
//! - a code is accepted once (no replay within its window),
//! - failed codes are capped across all clients: past the cap, code logins
//!   are refused until the hour rolls over (the password still works).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use sha1::Sha1;

const STEP_SECS: u64 = 30;
const DIGITS: u32 = 6;
/// Accept the previous and next step too (clock drift, slow typing).
const SKEW_STEPS: i64 = 1;
const GLOBAL_FAILURE_CAP: usize = 30;
const GLOBAL_FAILURE_WINDOW: Duration = Duration::from_secs(3600);

pub struct Totp {
	secret: Vec<u8>,
	last_used_step: Mutex<Option<u64>>,
	failures: Mutex<VecDeque<Instant>>,
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
	Accepted,
	Rejected,
	/// Too many failed codes overall: code logins are paused.
	Paused,
}

impl Totp {
	/// `secret` is base32 (spaces and case ignored), at least 80 bits.
	pub fn from_base32(secret: &str) -> Result<Self, String> {
		let cleaned: String = secret
			.chars()
			.filter(|c| !c.is_whitespace() && *c != '=')
			.map(|c| c.to_ascii_uppercase())
			.collect();
		let secret = BASE32_NOPAD
			.decode(cleaned.as_bytes())
			.map_err(|e| format!("OG_TOTP_SECRET is not valid base32: {e}"))?;
		if secret.len() < 10 {
			return Err("OG_TOTP_SECRET is too short (use og-server totp-setup)".into());
		}
		Ok(Self {
			secret,
			last_used_step: Mutex::new(None),
			failures: Mutex::new(VecDeque::new()),
		})
	}

	pub fn generate_secret() -> String {
		let mut bytes = [0u8; 20];
		getrandom::fill(&mut bytes).expect("OS random source");
		BASE32_NOPAD.encode(&bytes)
	}

	fn code_at(&self, step: u64) -> u32 {
		let mut mac = Hmac::<Sha1>::new_from_slice(&self.secret).expect("any key length");
		mac.update(&step.to_be_bytes());
		let digest = mac.finalize().into_bytes();
		let offset = (digest[19] & 0x0f) as usize;
		let binary = u32::from_be_bytes([
			digest[offset] & 0x7f,
			digest[offset + 1],
			digest[offset + 2],
			digest[offset + 3],
		]);
		binary % 10u32.pow(DIGITS)
	}

	fn step_now() -> u64 {
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|d| d.as_secs() / STEP_SECS)
			.unwrap_or(0)
	}

	/// Whether `input` looks like a code (so the gate tries TOTP first).
	pub fn looks_like_code(input: &str) -> bool {
		let digits: String = input.chars().filter(|c| !c.is_whitespace()).collect();
		digits.len() == DIGITS as usize && digits.chars().all(|c| c.is_ascii_digit())
	}

	pub fn verify(&self, input: &str) -> Verdict {
		self.verify_at(input, Self::step_now(), Instant::now())
	}

	fn verify_at(&self, input: &str, now_step: u64, now: Instant) -> Verdict {
		{
			let mut failures = self.failures.lock().unwrap();
			while failures
				.front()
				.is_some_and(|at| now.duration_since(*at) >= GLOBAL_FAILURE_WINDOW)
			{
				failures.pop_front();
			}
			if failures.len() >= GLOBAL_FAILURE_CAP {
				return Verdict::Paused;
			}
		}

		let digits: String = input.chars().filter(|c| !c.is_whitespace()).collect();
		let Ok(code) = digits.parse::<u32>() else {
			return self.fail(now);
		};
		let mut last_used = self.last_used_step.lock().unwrap();
		for skew in -SKEW_STEPS..=SKEW_STEPS {
			let Some(step) = now_step.checked_add_signed(skew) else { continue };
			if last_used.is_some_and(|used| step <= used) {
				continue; // already used (or older): no replay
			}
			if constant_time_eq(self.code_at(step), code) {
				*last_used = Some(step);
				return Verdict::Accepted;
			}
		}
		drop(last_used);
		self.fail(now)
	}

	fn fail(&self, now: Instant) -> Verdict {
		self.failures.lock().unwrap().push_back(now);
		Verdict::Rejected
	}

	/// `otpauth://` URI for authenticator apps (and the QR code).
	pub fn provisioning_uri(secret_base32: &str, label: &str) -> String {
		let label = label.replace(' ', "%20");
		format!(
			"otpauth://totp/Open%20Grind:{label}?secret={secret_base32}&issuer=Open%20Grind&algorithm=SHA1&digits={DIGITS}&period={STEP_SECS}"
		)
	}
}

fn constant_time_eq(a: u32, b: u32) -> bool {
	(a ^ b) == 0
}

/// QR code as terminal text (two pixel rows per line), for `totp-setup`.
pub fn qr_for_terminal(data: &str) -> Result<String, String> {
	use qrcode::render::unicode::Dense1x2;
	let code = qrcode::QrCode::new(data.as_bytes()).map_err(|e| e.to_string())?;
	Ok(code
		.render::<Dense1x2>()
		.dark_color(Dense1x2::Light)
		.light_color(Dense1x2::Dark)
		.quiet_zone(true)
		.build())
}

#[cfg(test)]
mod tests {
	use super::*;

	// RFC 6238 test secret "12345678901234567890" (SHA-1).
	const RFC_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

	fn totp() -> Totp {
		Totp::from_base32(RFC_SECRET).unwrap()
	}

	#[test]
	fn matches_the_rfc_6238_vectors() {
		let totp = totp();
		// times 59 and 1111111109 -> 8-digit 94287082 / 07081804; 6 digits:
		assert_eq!(totp.code_at(59 / STEP_SECS), 287_082);
		assert_eq!(totp.code_at(1_111_111_109 / STEP_SECS), 81_804);
	}

	#[test]
	fn accepts_a_code_once_with_one_step_of_skew() {
		let totp = totp();
		let now = Instant::now();
		let step = 1_000_000;
		let code = format!("{:06}", totp.code_at(step - 1));
		assert_eq!(totp.verify_at(&code, step, now), Verdict::Accepted);
		assert_eq!(totp.verify_at(&code, step, now), Verdict::Rejected, "replay");
		let far = format!("{:06}", totp.code_at(step - 3));
		assert_eq!(totp.verify_at(&far, step, now), Verdict::Rejected);
	}

	#[test]
	fn pauses_after_too_many_failures_and_resumes_after_the_window() {
		let totp = totp();
		let start = Instant::now();
		for _ in 0..GLOBAL_FAILURE_CAP {
			assert_eq!(totp.verify_at("000000", 5, start), Verdict::Rejected);
		}
		let good = format!("{:06}", totp.code_at(5));
		assert_eq!(totp.verify_at(&good, 5, start), Verdict::Paused);
		let later = start + GLOBAL_FAILURE_WINDOW;
		assert_eq!(totp.verify_at(&good, 5, later), Verdict::Accepted);
	}

	#[test]
	fn recognises_codes_and_reads_relaxed_base32() {
		assert!(Totp::looks_like_code("123 456"));
		assert!(!Totp::looks_like_code("hunter2"));
		assert!(Totp::from_base32("gezd gnbv gy3t qojq gezd gnbv gy3t qojq").is_ok());
		assert!(Totp::from_base32("ABC").is_err());
		assert!(Totp::provisioning_uri("ABC", "me").contains("secret=ABC"));
	}
}
