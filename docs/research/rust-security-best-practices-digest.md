# Digest: corgea.com/learn/rust-security-best-practices

Source: https://corgea.com/learn/rust-security-best-practices
Page title: "Rust Best Practices 2026: Security, Idioms & Error Handling"

The page targets stable Rust and the 2024 edition. Framing sentence: *"memory safe" differs from "secure," and "compiles" differs from "idiomatic."* Rust code still faces "logic mistakes, integer wraparound, injection vulnerabilities, resource exhaustion, and vulnerable dependencies."

Everything below is from the site unless explicitly marked **[not from site]**. Fetched 2026-09-04 via WebFetch in six passes (outline, first/middle/final thirds, type-driven/testing/perf, FAQ, verbatim code blocks).

---

## 1. Section headings, in order

1. **Project and tooling hygiene**
   - Format and lint on every change
   - Pin a minimum supported Rust version
   - Audit dependencies with cargo-audit and cargo-deny
   - Workspace layout
   - A minimal CI pipeline
2. **Ownership and borrowing idioms**
   - Prefer borrowing in function signatures
   - Treat clone as a design decision
   - Use Cow when ownership is conditional
   - Let lifetimes be inferred, but understand them
3. **Error handling**
   - Return Result and propagate with ?
   - thiserror for libraries, anyhow for applications
   - No unwrap in libraries or on untrusted paths
   - Have a panics policy
4. **Type-driven design**
   - Newtypes for domain identifiers
   - Parse, don't validate
   - Enums instead of booleans
   - Builders and typestate for complex construction
   - Use must_use and non_exhaustive
5. **Unsafe hygiene**
   - Minimize the surface
   - Document the invariant with a SAFETY comment
   - Encapsulate unsafe behind a safe API
   - Run Miri
6. **Concurrency and async**
   - Send and Sync are your friends
   - Channels versus shared state
   - Tokio pitfalls
   - Cancellation safety
7. **Rust security best practices**
   - Validate and sanitize all input
   - Integer overflow: release mode wraps
   - Denial of service and resource limits
   - Dependency supply chain
   - Secrets and sensitive data
   - Use vetted cryptography
   - Serde deserialization limits
   - Path traversal
   - Command injection
   - FFI boundaries
   - Where static analysis fits
8. **Testing**
   - Unit, integration, and doc tests
   - Property-based testing with proptest
   - Fuzzing with cargo-fuzz
   - Supporting tools
9. **Performance and release settings**
   - Release profile
   - Avoid allocation in hot loops
10. **Rust best practices checklist** (25 items)
11. **FAQ**
12. **Related reading**

---

## 2. Section-by-section actionable rules

### 2.1 Project and tooling hygiene

**Format and lint on every change**
- Run `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features -- -D warnings` in CI.
- Declare lints in the `[lints]` table in `Cargo.toml`, not on the command line. Suggested config:
  ```toml
  [lints.rust]
  unsafe_code = "forbid"
  missing_docs = "warn"
  missing_crate_level_docs = "warn"

  [lints.clippy]
  unwrap_used = "warn"
  expect_used = "warn"
  undocumented_unsafe_blocks = "deny"
  cast_possible_truncation = "warn"
  ```
- In workspaces, define once under `[workspace.lints]` and opt each crate in with `lints.workspace = true`.

**Pin a minimum supported Rust version**
- Set `rust-version` in `Cargo.toml`; use `resolver = "3"` (default in 2024 edition) for MSRV-aware resolution; test against both MSRV and current stable in CI.
  ```toml
  [package]
  rust-version = "1.85"
  edition = "2024"
  ```

**Audit dependencies with cargo-audit and cargo-deny**
- `cargo audit` checks `Cargo.lock` against the RustSec advisory database.
- `cargo deny` with a `deny.toml` enforces policy on advisories, licenses, duplicate versions, and crate sources. Run both in CI.
- Commit `Cargo.lock` for binaries; build with `cargo build --locked`.
- For higher assurance, `cargo vet` records human review of each crate version.
- Install: `cargo install cargo-audit cargo-deny --locked`
- Suggested `deny.toml`:
  ```toml
  [advisories]
  yanked = "deny"
  unmaintained = "workspace"

  [licenses]
  allow = ["MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "Unicode-3.0"]

  [bans]
  multiple-versions = "warn"
  wildcards = "deny"

  [sources]
  unknown-registry = "deny"
  unknown-git = "deny"
  ```

**Workspace layout**
- One library crate per bounded concern; thin binaries that only do "config + composition."
  ```
  .
  ├── Cargo.toml          # [workspace] members, shared deps, shared lints
  ├── crates/
  │   ├── core/           # domain types, no I/O
  │   ├── storage/        # database access
  │   └── api/            # HTTP handlers
  └── bin/
      └── server/         # main.rs: config + composition only
  ```
- Declare versions once in `[workspace.dependencies]` and reference with `dep = { workspace = true }`. Quote: "This keeps every crate on the same version of `serde` or `tokio`, which is both a build-time win and a security win."

**A minimal CI pipeline**
- Quote: "A Rust CI job should at least run `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --locked`, `cargo audit`, and `cargo deny check`, plus a Miri job if you have `unsafe`." Cache the target directory; keep the pipeline fast. No YAML is given.

### 2.2 Ownership and borrowing idioms

- Accept `&str` not `&String`, `&[T]` not `&Vec<T>`, `&Path` not `&PathBuf`. Take ownership only when the function keeps or consumes the value.
- Treat `.clone()` as a design decision, never a borrow-checker silencer. Before cloning ask: should it borrow, should it be `Rc`/`Arc`-shared, or should a struct hold a reference? Clippy lints `redundant_clone` and `needless_pass_by_value` catch mechanical cases.
- Use `Cow<'_, str>` when ownership is conditional: `Cow::Borrowed` when unchanged, `Cow::Owned` when modified. Quote: "This pattern is everywhere in parsers, sanitizers, and path normalizers, which are exactly the places where security-relevant code lives."
  ```rust
  use std::borrow::Cow;

  fn strip_bom(input: &str) -> Cow<'_, str> {
      match input.strip_prefix('\u{FEFF}') {
          Some(rest) => Cow::Borrowed(rest),
          None => Cow::Borrowed(input),
      }
  }

  fn escape_html(input: &str) -> Cow<'_, str> {
      if !input.contains(['<', '>', '&', '"']) {
          return Cow::Borrowed(input);
      }
      let mut out = String::with_capacity(input.len() + 8);
      for c in input.chars() {
          match c {
              '<' => out.push_str("&lt;"),
              '>' => out.push_str("&gt;"),
              '&' => out.push_str("&amp;"),
              '"' => out.push_str("&quot;"),
              _ => out.push(c),
          }
      }
      Cow::Owned(out)
  }
  ```
- Let lifetimes be inferred. Structs holding references suit short-lived views (iterators, parsers); long-lived state should own its data. "A struct with a lifetime parameter stored in an `Arc` usually should own fields instead."

### 2.3 Error handling

- Any function that can fail returns `Result<T, E>`; propagate with `?` (auto `From` conversion). The canonical example is a config loader:
  ```rust
  fn load_config(path: &Path) -> Result<Config, ConfigError> {
      let raw = fs::read_to_string(path)?;
      let cfg: Config = toml::from_str(&raw)?;
      cfg.validate()?;
      Ok(cfg)
  }
  ```
- `thiserror` in libraries, `anyhow` in applications:
  ```rust
  // In a library crate: typed, matchable errors.
  #[derive(Debug, thiserror::Error)]
  pub enum ConfigError {
      #[error("could not read config file")]
      Io(#[from] std::io::Error),
      #[error("config is not valid TOML")]
      Parse(#[from] toml::de::Error),
      #[error("field `{field}` is invalid: {reason}")]
      Invalid { field: &'static str, reason: String },
  }

  // In a binary: add context, bubble up, report once at the top.
  use anyhow::{Context, Result};

  fn main() -> Result<()> {
      let cfg = payments::load_config("config.toml".as_ref())
          .context("loading configuration")?;
      run(cfg).context("running server")
  }
  ```
- Never expose `anyhow::Error` from a library's public API: "Callers then cannot distinguish 'file not found' from 'permission denied' without string matching."
- No `unwrap`/`expect` in libraries or on untrusted paths. "In network services, this becomes a denial-of-service primitive." Reserve `expect("reason")` for invariants that "genuinely cannot fail by construction." Enforce with `clippy::unwrap_used` and `clippy::expect_used`; allow in tests via `#![allow(clippy::unwrap_used)]`.
  ```rust
  // Bad: crashes on missing/malformed header
  let host = req.headers().get("host").unwrap().to_str().unwrap();

  // Good: returns error instead
  let host = req.headers().get("host")
      .and_then(|v| v.to_str().ok())
      .ok_or(ApiError::BadRequest("missing host header"))?;
  ```
- Have a panics policy: "Decide, per binary, what a panic means" and document it. Servers: panics are bugs isolated to the task. Embedded/safety-critical: `panic = "abort"` is common. Do not use `std::panic::catch_unwind` as a general exception handler; reserve it for FFI boundaries and test harnesses.

### 2.4 Type-driven design

- Newtypes for identifiers so swapped arguments in authorization checks become compile errors. "Swapped-argument bugs in authorization checks are one of the most common ways an insecure direct object reference (IDOR) gets into a codebase, and a newtype makes them a compile error."
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub struct UserId(u64);

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub struct OrderId(u64);

  fn get_order(user: UserId, order: OrderId) -> Result<Order, StoreError> { /* ... */ }
  ```
- Parse, don't validate: parse untrusted input once into a type that can only hold valid values. "Downstream code that takes an `Email` never has to re-check it, and there is no way to construct one without going through `parse`. Keep the inner field private for exactly that reason."
  ```rust
  pub struct Email(String);

  impl Email {
      pub fn parse(raw: &str) -> Result<Self, ValidationError> {
          let trimmed = raw.trim();
          if trimmed.len() > 254 || !trimmed.contains('@') {
              return Err(ValidationError::Email);
          }
          Ok(Email(trimmed.to_ascii_lowercase()))
      }

      pub fn as_str(&self) -> &str { &self.0 }
  }
  ```
- Enums instead of booleans: "A function with two `bool` parameters has four combinations and no compiler help telling them apart." "When you add a fourth `Visibility` variant, every `match` in the codebase fails to compile until it handles it. That is the point."
  ```rust
  pub enum Visibility { Public, Private, Unlisted }
  pub enum Overwrite { Allow, Refuse }

  fn publish(doc: &Doc, vis: Visibility, overwrite: Overwrite) { /* ... */ }
  ```
- Builders for complex construction; typestate for order-dependent operations so "an unauthenticated send cannot compile."
  ```rust
  pub struct Request<State> { inner: RequestParts, _state: std::marker::PhantomData<State> }
  pub struct NoAuth;
  pub struct Authed;

  impl Request<NoAuth> {
      pub fn authenticate(self, token: &Token) -> Result<Request<Authed>, AuthError> { /* ... */ }
  }

  impl Request<Authed> {
      pub fn send(self) -> Result<Response, SendError> { /* ... */ }
  }
  ```
- "Mark functions whose return value carries meaning with `#[must_use]`, so ignoring the result is a warning. Mark public enums and structs that will grow with `#[non_exhaustive]`."

### 2.5 Unsafe hygiene

- Keep each `unsafe` block as short as possible, ideally one operation.
- `#![forbid(unsafe_code)]` at the top of every crate that does not need `unsafe`. In workspaces, only FFI and low-level crates should allow it.
- Every `unsafe` block gets a `// SAFETY:` comment naming the specific invariant. Enforced by clippy `undocumented_unsafe_blocks`.
  ```rust
  pub fn first_byte(buf: &[u8]) -> Option<u8> {
      if buf.is_empty() {
          return None;
      }
      // SAFETY: we checked above that `buf` has at least one element,
      // so index 0 is in bounds.
      Some(unsafe { *buf.get_unchecked(0) })
  }
  ```
- Encapsulate behind a safe API: public functions must be safe to call with any arguments; all checking happens inside the wrapper. If soundness depends on the caller, mark it `unsafe fn` with a `# Safety` doc section. Since Rust 1.81 `unsafe_op_in_unsafe_fn` warns by default, so `unsafe` blocks (with `SAFETY` comments) are needed even inside `unsafe fn`.
- Run Miri on crates containing `unsafe` (not the whole workspace). It detects "out-of-bounds access, use-after-free, invalid aliasing, uninitialized reads, and data races in single-threaded code."
  ```
  rustup +nightly component add miri
  cargo +nightly miri test -p lowlevel-crate
  ```
- `cargo geiger` counts `unsafe` usage across the dependency tree.

### 2.6 Concurrency and async

- "`Send` means a value can move to another thread; `Sync` means it can be shared by reference. Both are auto traits, so most types get them for free. When the compiler says a future is not `Send`, you are almost always holding a non-`Send` value across an `.await`. Fix the ownership; do not reach for `unsafe impl Send`."
- Shared state: `Arc<Mutex<T>>` for "small, short-lived critical sections on state that several parties genuinely need to read and write." Channels when one party produces and another consumes; they "make ownership transfer explicit and are much harder to deadlock."
- `std::thread::scope` for borrowing from the stack; `rayon` for data parallelism.
- Tokio pitfalls:
  - "Do not hold `std::sync::Mutex` across an `.await`. The guard is not `Send`, and even where it compiles, it blocks every other task on the same worker thread. Scope the lock so it drops before the `.await`, or use `tokio::sync::Mutex`."
  - "CPU-heavy work, synchronous file I/O, and blocking clients belong in `tokio::task::spawn_blocking`. A single blocking call in a handler stalls every task sharing that worker."
  - "Dropping a `JoinHandle` does not cancel the task. It detaches."
  - "Put a timeout on everything that talks to the network. `tokio::time::timeout(dur, fut)` is one line and turns an unbounded hang into a handled error."
- Cancellation safety: a future is cancellation-safe if "dropping it partway through does not lose data or leave state inconsistent."

### 2.7 Rust security best practices

**Validate and sanitize all input**
- "Treat every byte from a user, a file, the network, or an environment variable as hostile until you have parsed it into a typed value."
- "Validate lengths, ranges, character sets, and structure at the boundary, and reject rather than 'clean' wherever you can."
- Allowlists over denylists:
  ```rust
  // Weak: a denylist misses encodings, unicode lookalikes, and whatever you forgot.
  fn strip_shell_metachars(data: &str) -> String {
      data.replace(|c: char| matches!(c, ';' | '|' | '"' | '&' | '$' | '`'), "")
  }

  // Strong: an allowlist that says exactly what is acceptable.
  fn parse_username(raw: &str) -> Result<Username, ValidationError> {
      let ok = (3..=32).contains(&raw.len())
          && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
      ok.then(|| Username(raw.to_owned())).ok_or(ValidationError::Username)
  }
  ```
- "Prefer parameterized queries for SQL (`sqlx`, `diesel`, and `rusqlite` all support them), typed builders for HTML, and structured APIs" generally "so the question of escaping never arises."

**Integer overflow: release mode wraps**
- "Rust panics on integer overflow in debug builds, but in release builds arithmetic wraps silently by default." An attacker controlling a length or count can wrap a calculation to a tiny number and bypass a bounds check. Cited: CVE-2018-1000810 in `str::repeat`.
- Defense 1:
  ```toml
  [profile.release]
  overflow-checks = true
  ```
- Defense 2: explicit methods on untrusted arithmetic:
  ```rust
  fn total_size(count: usize, item_len: usize) -> Option<usize> {
      count.checked_mul(item_len)?.checked_add(HEADER_LEN)
  }

  let capped = requested.saturating_add(padding);   // clamps at usize::MAX
  let hash = seed.wrapping_mul(0x9E37_79B9);        // wrapping is the intent here
  ```
- "Use `try_from` when narrowing (`u64` to `u32`, `usize` to `i32`) instead of `as`, which truncates silently." Lint: `clippy::cast_possible_truncation`.

**Denial of service and resource limits**
- "Memory safety does not stop an attacker from making you allocate 4 GB. Any place where an untrusted number drives an allocation, a loop, or a recursion depth needs a bound."
  ```rust
  const MAX_ITEMS: usize = 10_000;

  // Bad: `len` comes straight from the wire.
  let mut items = Vec::with_capacity(len);

  // Good: cap it before it touches the allocator.
  if len > MAX_ITEMS {
      return Err(ProtocolError::TooManyItems(len));
  }
  let mut items = Vec::with_capacity(len);
  ```
- "Set request body limits and per-connection timeouts in your HTTP framework (`axum`, `actix-web`, and `hyper` all expose them)."
- "Cap decompression output, because a 1 MB gzip bomb can expand to gigabytes."
- "Use the `regex` crate rather than a backtracking engine; it guarantees linear-time matching, which removes an entire class of ReDoS."
- "For `serde_json`, note that the default recursion limit protects you from deeply nested documents, but a hostile 100 MB array of small objects is still your problem, so read into a bounded buffer first."

**Dependency supply chain**
- "A Rust binary is typically 90 percent other people's code."
- "`build.rs` and proc macros run at compile time ... A malicious `build.rs` owns your developer laptop and your CI runner before you ship anything. This is why `cargo vet` and reviewing new dependencies matter even for 'just a dev dependency.'"
- Typosquats: "Check the crate name, download count, repository link, and maintainer before adding anything."
- "Prefer well-maintained crates with few transitive dependencies. `cargo tree` shows what you are really pulling in."
- "Commit `Cargo.lock`, build with `--locked`, and consider `cargo auditable` to embed the dependency list in the binary so you can answer 'is this deployed artifact affected?' later."
- Tools: `cargo audit`, `cargo deny`, `cargo vet`, `cargo tree`, `cargo geiger`, `cargo auditable`.

**Secrets and sensitive data**
- Crates: `secrecy`, `zeroize`, `subtle`.
  ```rust
  use secrecy::{ExposeSecret, SecretString};
  use zeroize::Zeroizing;

  pub struct DbConfig {
      pub url: String,
      pub password: SecretString,   // Debug prints "[REDACTED]", never the value
  }

  fn derive_key(passphrase: &str) -> Zeroizing<[u8; 32]> {
      let mut key = Zeroizing::new([0u8; 32]);
      // ... fill `key` ...
      key                            // zeroed on drop, even on early return
  }
  ```
- Three habits: (1) wrap secrets in types whose `Debug` redacts, never derive `Debug` on raw secrets; (2) use `zeroize` so key material is wiped on drop; (3) compare with constant-time `subtle::ConstantTimeEq`, not `==`.

**Use vetted cryptography**
- "Do not write your own. The Rust ecosystem has audited implementations." RustCrypto family (`aes-gcm`, `chacha20poly1305`, `sha2`, `hmac`, `argon2`), `ring` "for a batteries-included primitive set," `rustls` for TLS.
- Randomness: `rand::rngs::OsRng` or `getrandom`.
- Passwords: `argon2::Argon2::default().hash_password(...)` with random salt via the `password-hash` API, not `Sha256`.
  ```rust
  use sha2::{Digest, Sha256};
  let digest = Sha256::digest(b"content to fingerprint");
  println!("sha256 = {digest:x}");
  ```

**Serde deserialization limits**
  ```rust
  #[derive(serde::Deserialize)]
  #[serde(deny_unknown_fields)]
  pub struct CreateUser {
      #[serde(deserialize_with = "bounded_string::<64, _>")]
      pub username: String,
      pub email: String,
      #[serde(default)]
      pub roles: Vec<Role>,      // an enum, so unknown roles are rejected
  }
  ```
- `deny_unknown_fields` "blocks mass-assignment style attacks where a client sneaks in `"is_admin": true`."
- "Bounding string and collection sizes in a `deserialize_with` function, or validating right after deserialization, keeps a single request from becoming a memory bomb."
- "Use enums for anything with a fixed set of values so that invalid variants fail at parse time."

**Path traversal**
- "`Path::join` has a sharp edge: joining an absolute path replaces the base entirely, so `base.join(user_input)` with `user_input = "/etc/passwd"` returns `/etc/passwd`."
  ```rust
  use std::path::{Component, Path, PathBuf};

  fn safe_join(base: &Path, untrusted: &str) -> Result<PathBuf, FsError> {
      let rel = Path::new(untrusted);
      if rel.is_absolute()
          || rel.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
      {
          return Err(FsError::Traversal);
      }
      let full = base.join(rel).canonicalize()?;
      let base = base.canonicalize()?;
      full.starts_with(&base).then_some(full).ok_or(FsError::Traversal)
  }
  ```
- "Rejecting `..` components before joining, then confirming with `canonicalize` and `starts_with`, handles symlinks and platform-specific prefixes."
- "Remember that `canonicalize` requires the file to exist; for paths you are about to create, check the parent instead."

**Command injection**
  ```rust
  use std::process::Command;

  // Bad: user input becomes shell syntax.
  Command::new("sh").arg("-c").arg(format!("convert {input} out.png")).status()?;

  // Good: each argument is passed to the program directly.
  Command::new("convert").arg(&input).arg("out.png").status()?;
  ```
- "`std::process::Command` does not go through a shell, so arguments passed with `.arg()` are never interpreted for metacharacters."
- "Even with `.arg()`, validate the input against what the target program expects. A filename beginning with `-` can still be parsed as a flag by the child process, so prefix with `--` or reject leading dashes."

**FFI boundaries**
- "Validate every pointer for null and every length against the buffer it describes before touching memory."
- "Use `CStr`/`CString` for strings and never assume a C string is UTF-8; `CStr::to_str()` returns a `Result` for a reason."
- "Do not let a panic unwind into C. Since Rust 1.81, a panic escaping an `extern "C"` function aborts the process; if the C side can handle unwinding, declare `extern "C-unwind"`, otherwise wrap the body in `std::panic::catch_unwind` and return an error code."
- "Prefer `bindgen` for generating declarations and `cxx` for C++ so signatures are derived from headers rather than typed by hand."
- "Document who owns each allocation and which side frees it; mismatched allocators are a classic FFI crash."

**Where static analysis fits**
- "The compiler catches memory errors. Clippy catches idiom errors. Neither one knows that `get_order` should have checked that the user owns the order, or that the value flowing into `Command::arg` came from an HTTP query string." SAST closes "the gap between 'memory safe' and 'secure.'" Vendor pitch for Corgea's AI SAST, dependency/secret scanning (Rust support added August 20, 2026), and their Rust-written Sighthound scanner follows.

### 2.8 Testing

- Unit tests in `#[cfg(test)] mod tests` next to the code; integration tests in `tests/` against public API only; doc examples on public functions ("Cargo test compiles and runs them, so they never rot").
- `proptest`: "Property tests generate thousands of inputs and check an invariant, then shrink any failure to a minimal reproduction."
  ```rust
  use proptest::prelude::*;

  proptest! {
      #[test]
      fn safe_join_never_escapes_base(input in "\\PC{0,64}") {
          let base = std::env::temp_dir();
          if let Ok(p) = safe_join(&base, &input) {
              prop_assert!(p.starts_with(base.canonicalize().unwrap()));
          }
      }

      #[test]
      fn parse_then_display_roundtrips(email in "[a-z]{1,10}@[a-z]{1,10}\\.com") {
          let parsed = Email::parse(&email).unwrap();
          prop_assert_eq!(parsed.as_str(), email.to_ascii_lowercase());
      }
  }
  ```
- `cargo fuzz` (libFuzzer): "Fuzzing feeds random and mutated bytes into a function for hours and reports crashes, hangs, and (under sanitizers) memory errors. Anything that parses bytes from the outside world (file formats, protocol frames, config) should have a fuzz target." Targets "must never panic or hang."
  ```
  cargo install cargo-fuzz
  cargo fuzz init
  cargo +nightly fuzz run parse_packet
  ```
  ```rust
  #![no_main]
  use libfuzzer_sys::fuzz_target;

  fuzz_target!(|data: &[u8]| {
      let _ = mypacket::parse(data);   // must never panic or hang
  });
  ```
- Supporting tools: `cargo nextest` (faster runner), `cargo llvm-cov` (coverage), `insta` (snapshot tests), `loom` (thread interleavings).

### 2.9 Performance and release settings

```toml
[profile.release]
lto = "fat"            # or "thin" for a faster build with most of the benefit
codegen-units = 1
opt-level = 3
strip = "symbols"      # smaller binary; keep debug = 1 instead if you profile in prod
overflow-checks = true # from the security section; the cost is small
```
- "For a shipping binary, turn on link-time optimization and single codegen unit; expect a noticeably smaller and faster artifact at the cost of a slower release build."
- Keep `panic = "unwind"` unless you have a specific reason to abort.
- Hot loops: "Reuse buffers with `clear()`, prefer lazy iterator chains over intermediate collections, slice `&str` instead of creating `String`s, and pre-size with `with_capacity`."
- Measure with `criterion` and `cargo flamegraph` first; "if the profile says the time is in `clone`, go back to the ownership section."

### 2.10 FAQ

- **What are the best practices for Rust?** "Run `cargo fmt` and Clippy in CI, pin an MSRV, prefer borrowing over cloning, return `Result` and propagate with `?`, model your domain with newtypes and enums, keep `unsafe` small and documented, use channels or tightly scoped locks for concurrency, audit dependencies with `cargo audit` and `cargo deny`, validate all input, handle integer overflow explicitly, and test with unit, property, and fuzz tests."
- **Is Rust secure by default?** "Rust is memory-safe by default, which removes buffer overflows, use-after-free, and data races from safe code. It is not secure by default in the application sense: nothing in the language prevents injection, path traversal, broken authorization, silent integer wraparound in release builds, resource exhaustion, secret leakage, or a vulnerable dependency."
- **Should I use unwrap in Rust?** Not in library code or on paths processing external input; it turns recoverable errors into panics, a DoS vector in services. Use `?` to propagate and `expect("reason")` only for invariants proven impossible to violate. `clippy::unwrap_used` enforces it.
- **anyhow vs thiserror?** "Use `thiserror` for libraries, where callers need typed error variants they can match on. Use `anyhow` for applications, where you want to add context and report once at the top." Most projects use both, converting library errors to `anyhow::Error` at the binary boundary.
- **How do I audit Rust dependencies?** "`cargo audit` checks `Cargo.lock` against the RustSec advisory database. `cargo deny` enforces a policy for advisories, licenses, duplicate versions, and allowed sources." Commit `Cargo.lock`, build `--locked`, use `cargo vet` when human review needs recording.
- **How do I write safe unsafe Rust?** "Make each `unsafe` block as small as possible, write a `// SAFETY:` comment naming the invariant that makes it sound, and wrap it in a safe function whose signature makes the invariant impossible to violate."

**Related reading:** three Corgea marketing articles (AI Code Security 2026; 10 Best AI Code Security Tools 2026; AI SAST guide).

---

## 3. The "Rust best practices checklist" (25 items)

Reproduced as returned by WebFetch; wording is faithful to the page.

1. Run `cargo fmt --check` and `cargo clippy -- -D warnings` in CI, with lints declared in `[lints]`.
2. Set `rust-version` in `Cargo.toml` and test against the MSRV.
3. Run `cargo audit` and `cargo deny check` on every build; commit `Cargo.lock` and build with `--locked`.
4. Accept `&str`, `&[T]`, and `&Path` in signatures; take ownership only when you need it.
5. Treat every `clone()` as a design decision; use `Cow` when ownership is conditional.
6. Return `Result` and propagate with `?`; `thiserror` in libraries, `anyhow` in binaries.
7. Enable `clippy::unwrap_used`; reserve `expect` for true invariants with a stated reason.
8. Use newtypes for identifiers, enums instead of booleans, and private fields so invalid values cannot be constructed.
9. Add `#[must_use]` to results that matter and `#[non_exhaustive]` to public types that will grow.
10. Keep `unsafe` blocks tiny, write `// SAFETY:` comments, wrap them in safe APIs, and run Miri.
11. `#![forbid(unsafe_code)]` in every crate that does not need it.
12. Never hold a `std::sync::Mutex` guard across `.await`; use `spawn_blocking` for blocking work; put timeouts on network calls.
13. Prefer channels for ownership transfer and short-lived locks for shared state.
14. Parse untrusted input into typed values at the boundary with allowlists, not denylists.
15. Set `overflow-checks = true` in release and use `checked_`/`saturating_`/`wrapping_` deliberately; use `try_from` instead of `as` when narrowing.
16. Cap every untrusted length, body size, decompression output, and recursion depth.
17. Use RustCrypto, `ring`, or `rustls`; `OsRng` for randomness; `argon2` for passwords.
18. Wrap secrets in `secrecy`/`zeroize` types; never derive `Debug` on them; compare with `subtle`.
19. Add `#[serde(deny_unknown_fields)]` and size bounds on externally sourced types.
20. Reject `..` and absolute paths, then verify with `canonicalize` and `starts_with`.
21. Pass arguments to `Command` with `.arg()`; never build shell strings.
22. Validate pointers, lengths, and strings at FFI boundaries; never unwind a panic into C.
23. Write property tests with `proptest` and fuzz every parser with `cargo fuzz`.
24. Enable LTO and `codegen-units = 1` for release; profile before optimizing anything else.
25. Run SAST on pull requests to catch the logic, authorization, and data-flow bugs the compiler cannot.

---

## 4. Rules most relevant to a filesystem-walking CLI that parses markdown/source via tree-sitter (C FFI), reads TOML config, and emits JSON

Ordered roughly by how directly they bite this kind of project. Checklist numbers refer to section 3.

### Untrusted file input is the threat model
The site's framing applies verbatim: "Treat every byte from a user, **a file**, the network, or an environment variable as hostile until you have parsed it into a typed value." Every markdown/source file walked and every config file read is an untrusted input. Everything below follows from that.

### Panics vs Result (checklist 6, 7)
- A CLI that walks thousands of files must not die on one malformed file. Site rule: no `unwrap`/`expect` "on paths processing external input"; return `Result`, propagate with `?`.
- The site's library/binary split maps directly: `thiserror` enums in the library crates (walker, config, parser adapters), `anyhow` with `.context("parsing <path>")` at the binary top. Never leak `anyhow::Error` from library crates.
- Enable `clippy::unwrap_used` / `clippy::expect_used` in `[lints.clippy]`; allow in tests.
- Have and document a panics policy per binary. For this tool: a panic during one file's parse should be reported and the file skipped, not crash the run. The site reserves `catch_unwind` for "FFI boundaries and test harnesses," which is exactly where tree-sitter sits, so isolating per-file parse calls with `catch_unwind` is consistent with the site.
- Keep `panic = "unwind"` (site's default recommendation) rather than `abort`, since per-file isolation depends on unwinding.

### Config loading is the site's own example (checklist 6, 8, 14, 19)
- The `load_config` snippet (`fs::read_to_string` -> `toml::from_str` -> `cfg.validate()`) and the `ConfigError { Io(#[from] io::Error), Parse(#[from] toml::de::Error), Invalid { field, reason } }` enum come straight from the page; adopt them.
- `#[serde(deny_unknown_fields)]` on the config struct so misspelled keys fail loudly instead of being silently ignored.
- Use enums for fixed-choice config fields so "invalid variants fail at parse time."
- Bound string/collection sizes in `deserialize_with` or validate immediately after deserialization.
- "Parse, don't validate": turn config into newtypes with private fields once (e.g. a validated root path, a validated glob set), then never re-check downstream.

### Path handling / traversal (checklist 4, 20)
Relevant wherever a path comes from config (roots, include/exclude patterns, output path) or from file contents (markdown links, include directives, anchors referencing other files).
- `Path::join` with an absolute right-hand side discards the base.
- Reject `is_absolute()` and any `Component::ParentDir` / `Component::Prefix(_)` before joining.
- Then `canonicalize()` both sides and check `starts_with`. This "handles symlinks and platform-specific prefixes."
- `canonicalize` requires existence; for output paths you are about to create, canonicalize and check the parent.
- Accept `&Path` in signatures rather than `&PathBuf`.
- The site's proptest for `safe_join` ("never escapes base" over `\PC{0,64}` input) is a ready-made property test to port.

### Unsafe / FFI boundary with tree-sitter (checklist 10, 11, 22)
- `#![forbid(unsafe_code)]` in every crate except the one that wraps tree-sitter. Site: "In workspaces, most crates should forbid `unsafe`; only FFI and low-level crates should allow it." This argues for an isolated tree-sitter adapter crate in the workspace.
- **[not from site]** The `tree-sitter` Rust crate already wraps the C API safely, so you may have zero `unsafe` of your own; the rules below still govern any raw-API use, dynamic grammar loading, or custom C bindings.
- Every `unsafe` block: tiny, `// SAFETY:` comment, `undocumented_unsafe_blocks = "deny"`, wrapped in a safe API whose signature makes misuse impossible.
- Strings across FFI: "never assume a C string is UTF-8; `CStr::to_str()` returns a `Result` for a reason." Source files from disk may not be UTF-8 either. **[not from site]** Decide at the boundary whether to reject, lossy-convert, or skip non-UTF-8 files, and make that a typed error rather than a panic.
- "Do not let a panic unwind into C." Any Rust callback handed to tree-sitter (custom read callbacks, loggers) must not panic; since 1.81 that aborts the process. Wrap such bodies in `catch_unwind` and return an error code.
- "Document who owns each allocation and which side frees it." Relevant if tree-sitter `Tree`/`Node` objects outlive their source text in your data structures.
- Run Miri on the crate containing `unsafe` (`cargo +nightly miri test -p <ffi-crate>`); use `cargo geiger` to measure how much `unsafe` tree-sitter and grammar crates bring in.
- Prefer `bindgen` over hand-typed C declarations.

### Integer overflow and narrowing (checklist 15)
Byte offsets, line/column numbers, and node counts come out of tree-sitter as C-width integers and get turned into JSON numbers and slice indices.
- `overflow-checks = true` in `[profile.release]` ("the cost is small").
- `checked_*`/`saturating_*` on arithmetic driven by file-derived values (span math like `end - start`, `offset + length` before slicing).
- `try_from`, not `as`, when narrowing (e.g. `usize` -> `u32` for JSON output, or `u32` -> `usize` for indexing). Lint `cast_possible_truncation = "warn"`.
- **[not from site]** Slicing a `&str` at a tree-sitter byte offset that is not a char boundary panics; treat offsets as untrusted and use `get(..)`.

### Denial of service via huge or pathological inputs (checklist 16)
"Any place where an untrusted number drives an allocation, a loop, or a recursion depth needs a bound." For this tool:
- Cap file size before `read_to_string` (a multi-GB file inside the walk root).
- Cap recursion depth when descending directories, when walking the syntax tree, and for nested markdown structures.
- Cap the number of files, nodes, and anchors collected; cap any `Vec::with_capacity(n)` where `n` derives from file contents.
- Use the `regex` crate for any content matching (headings, links, anchor slugs) because it "guarantees linear-time matching."
- If JSON is ever read back in: "a hostile 100 MB array of small objects is still your problem, so read into a bounded buffer first."
- "Cap decompression output" applies only if compressed inputs are ever handled.

### Command injection (checklist 21)
Only relevant if the tool shells out (e.g. to `git` for the repo root or ignore rules). Use `Command::new("git").arg(...)`, never `sh -c`. Because walked file paths can begin with `-`, "prefix with `--` or reject leading dashes."

### Allowlists for anchors and identifiers (checklist 14)
For anything parsed into an identifier, anchor slug, or filename, validate with an allowlist of characters plus a length range (the `(3..=32).contains(&raw.len()) && chars().all(...)` pattern) and "reject rather than 'clean.'" Denylists "miss encodings, unicode lookalikes, and whatever you forgot."

### Dependency auditing / supply chain (checklist 3)
Tree-sitter grammar crates compile C via `build.rs`; the site's warning is pointed: "A malicious `build.rs` owns your developer laptop and your CI runner before you ship anything."
- Vet each grammar crate (name, downloads, repo, maintainer) before adding it.
- `cargo tree` to see the transitive weight.
- Commit `Cargo.lock`; `cargo build --locked`.
- `cargo audit` and `cargo deny check` in CI with the site's `deny.toml`; `unknown-git = "deny"` matters because grammar crates are often pulled from git.
- Consider `cargo vet` and `cargo auditable` for a distributed binary.

### Secrets (checklist 18)
A filesystem walker can ingest `.env` files, private keys, or configs with tokens and then echo them into JSON output or logs. The site's rule: wrap secret-bearing values in redacting types and never derive `Debug` on them. **[not from site]** For this tool that means being deliberate about which file contents are emitted into JSON, and honoring ignore rules for sensitive files.

### Testing (checklist 23)
- Site: "Anything that parses bytes from the outside world (file formats, protocol frames, config) should have a fuzz target" that "must never panic or hang." Fuzz the markdown parser, the TOML config loader, and the per-file analysis entry point with `cargo fuzz`.
- Property-test `safe_join` with `proptest` as the site does.
- `insta` snapshot tests fit JSON output; `cargo nextest` and `cargo llvm-cov` as supporting tools.

### Release profile (checklist 24)
For a distributed CLI: `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`, `strip = "symbols"`, `overflow-checks = true`, `panic = "unwind"`.

### Ownership idioms for a parser-heavy tool (checklist 4, 5)
- "Structs holding references suit short-lived views (iterators, parsers); long-lived state should own its data."
- `Cow<'_, str>` for normalization passes (BOM stripping, escaping): "this pattern is everywhere in parsers, sanitizers, and path normalizers."
- Hot loops: reuse buffers with `clear()`, slice `&str` rather than build `String`s, pre-size with `with_capacity` (bounded, per the DoS rule).

### Concurrency (checklist 12, 13)
If files are processed in parallel: `rayon` for data parallelism, `std::thread::scope` for borrowing from the stack, and channels to hand results to a single JSON writer rather than a shared `Mutex<Vec<_>>`, since channels "make ownership transfer explicit and are much harder to deadlock." Not an async project, so the tokio pitfalls do not apply.

### Gaps the page does not cover that this project needs **[not from site]**
- Symlink loops and cycle detection during directory walking (the site only says `canonicalize` "handles symlinks" for traversal checks).
- File-descriptor limits and parallel open-file caps.
- Unicode normalization and char-boundary slicing.
- Anything tree-sitter specific (grammar loading, query safety, `Node` lifetimes).
- JSON output encoding of file-derived strings (escaping, invalid UTF-8, control characters).
