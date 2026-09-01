# Chapter 1 — Workspace and first run

**Goal.** An empty folder becomes a Cargo *workspace* with two crates: a
library `work-time-core` (still empty) and a binary `work-time-tracker` that
prints one line. Every later chapter adds code to these two crates.

**You will learn**

- What a package, a crate and a workspace are, and why this project has two crates.
- How `Cargo.toml` is organised: shared metadata, a dependency catalog, release settings.
- The five Cargo commands you will run in every chapter.
- What the lint settings forbid, and why a "no `unwrap`" rule makes better code.

**Prerequisite.** `git init` done in `time_tracker_study` (see `00-outline.md`).

---

## 1.1 The workspace manifest

Create `Cargo.toml` at the project root:

```toml
# Cargo.toml
# Virtual workspace: the repository root is not a crate itself.
[workspace]
members = ["crates/core", "crates/app"]
resolver = "3"

# Values shared by every member crate that opts in with `key.workspace = true`.
[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "GPL-3.0-or-later"
authors = ["majamato"]
repository = "https://github.com/majamato/work-time-tracker"

# Dependency catalog. Listing a crate here downloads nothing; a member crate
# opts in with `name.workspace = true`.
[workspace.dependencies]
chrono = { version = "0.4.41", features = ["serde"] }
csv = "1.3.1"
directories = "6.0.0"
gio = "0.21.2"
glib = "0.21.3"
gtk = { package = "gtk4", version = "0.10.1", features = ["v4_12"] }
libadwaita = { version = "0.8.0", features = ["v1_5"] }
proptest = "1.7.0"
rusqlite = { version = "0.37.0", features = ["backup", "bundled"] }
serde = { version = "1.0.219", features = ["derive"] }
serde_json = "1.0.143"
tempfile = "3.21.0"
thiserror = "2.0.16"
tracing = "0.1.41"
tracing-subscriber = { version = "0.3.20", features = ["env-filter"] }

[profile.release]
lto = "thin"
codegen-units = 1
strip = "debuginfo"
```

**What.** This file has no `[package]` section, so the root folder is not a
crate; it only lists which folders are. That is called a *virtual workspace*.

**Why.** The program is split in two:

- `crates/core` — the rules of time tracking (what a project is, when a timer
  may stop, how idle time is reconciled). Pure Rust, no GTK, no database.
- `crates/app` — everything that touches the outside world: SQLite, files,
  threads, the GTK window, D-Bus.

The split is the single most important design decision in the project. The
core can be tested in milliseconds without a graphical session, and the
compiler guarantees it cannot accidentally depend on GTK because that crate
simply does not list GTK as a dependency.

**Rust — package, crate, workspace.** A *crate* is Rust's unit of compilation:
one library or one executable. A *package* is a folder with a `Cargo.toml`
that builds one or more crates (at most one library, any number of binaries).
A *workspace* is a set of packages that share one `Cargo.lock` and one
`target/` build folder, so common dependencies are compiled once.

**Rust — the catalog.** `[workspace.dependencies]` is only a list of versions.
Nothing is downloaded until a member crate writes `serde.workspace = true`.
Keeping versions in one place means two crates can never disagree about which
`serde` they use. `"0.4.41"` means "0.4.41 or any later 0.4.x" (a *caret*
requirement, the Cargo default). `features = [...]` turns on optional parts of
a dependency; for example `serde`'s `derive` feature gives you the
`#[derive(Serialize)]` macro.

**Rust — editions and MSRV.** `edition = "2024"` selects the newest set of
language rules (each edition can change small defaults without breaking old
crates). `rust-version = "1.85"` is the *minimum supported Rust version*:
Cargo refuses to build with an older compiler and gives a clear message instead
of a confusing syntax error. `resolver = "3"` is the dependency resolver that
understands `rust-version`.

**Rust — release profile.** `cargo build --release` uses `[profile.release]`.
`lto = "thin"` lets the optimiser look across crate boundaries;
`codegen-units = 1` gives it the whole crate at once (slower compile, faster
code); `strip = "debuginfo"` removes debugging tables from the final binary.
You do not need to remember these; they are the common choice for desktop
applications.

## 1.2 Formatter settings and `.gitignore`

```toml
# rustfmt.toml
edition = "2024"
use_field_init_shorthand = true
```

```gitignore
# .gitignore
/target/
/build/
/build-meson/
/build-release/
/stage/
/.cache/

# Rust
**/*.rs.bk
*.pdb

# Native Linux build artifacts
compile_commands.json
*.o
*.obj
*.a
*.so
*.so.*
*.la
.deps/
.dirstamp
/autom4te.cache/
/config.log
/config.status

# Debugging and editor temporary files
core
*.core
.gdb_history
*.swp
*.swo
*~

# Packaging archives
*.rpm
*.tar.xz
*.tar.gz
```

**What.** `rustfmt` is Rust's formatter; every Rust project runs it, so all
Rust code looks the same. `use_field_init_shorthand` rewrites
`Project { name: name }` as `Project { name }` (you will see that shorthand in
Chapter 2). `target/` is Cargo's build output; `build/` and `stage/` are
Meson outputs used from Chapter 20.

**Idiom.** Never argue about formatting; run `cargo fmt` before every commit.

## 1.3 The core library crate

```toml
# crates/core/Cargo.toml
[package]
name = "work-time-core"
description = "Pure domain model and timer state machine for Work Time Tracker"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]

[dev-dependencies]

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
```

```rust
// crates/core/src/lib.rs
//! Domain rules for Work Time Tracker.
//!
//! This crate knows nothing about GTK, SQLite, D-Bus, or the filesystem.
```

**What.** A package named `work-time-core` whose only source file, `lib.rs`,
contains a documentation comment and nothing else. `version.workspace = true`
means "take the value from the root `[workspace.package]`".

**Rust — `lib.rs`.** A file named `src/lib.rs` makes the package a *library
crate*. Other crates import it with `use work_time_core::...` — note that the
hyphen in the package name becomes an underscore in Rust code, because `-` is
the minus operator.

**Rust — doc comments.** `//!` documents the item that *contains* the comment
(here: the whole crate). `///` (three slashes, coming in Chapter 2) documents
the item that *follows* it. `cargo doc` turns both into HTML. Plain `//`
comments are ignored by tooling.

**Rust — `[dependencies]` vs `[dev-dependencies]`.** Dev-dependencies are
only compiled for tests and examples; users of the library never download
them. Both sections are empty for now.

**Rust — lints.** A lint is a compile-time check beyond "does it type-check".

- `unsafe_code = "forbid"` — the `unsafe` keyword is not allowed anywhere in
  this crate. `unsafe` lets you bypass the borrow checker; a time tracker has
  no reason to.
- `unwrap_used` / `expect_used = "deny"` — these are Clippy lints (Clippy is
  Cargo's linter, run with `cargo clippy`). `.unwrap()` takes the value out of
  an `Option` or `Result` and *crashes the program* if it is empty or an error.
  Denying it forces every "this cannot fail" assumption to be written as a real
  decision: return an error, use a default, or explain with `unwrap_or_else`.
  You will see how this shapes the code from Chapter 2 on.

**Idiom.** Library code returns errors; it does not panic. Panics
(`unwrap`, `expect`, `panic!`) are acceptable in tests, where a failure *is* a
panic.

## 1.4 The application binary crate

```toml
# crates/app/Cargo.toml
[package]
name = "work-time-tracker"
description = "A local-first GNOME work time tracker"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
work-time-core = { path = "../core" }

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
```

```rust
// crates/app/src/main.rs
fn main() {
    println!("Work Time Tracker (study build)");
}
```

**What.** A second package. `src/main.rs` makes it a *binary crate*: Cargo
builds an executable named after the package, `work-time-tracker`. Its one
dependency is the core library, referenced by relative path instead of a
version number because it lives in the same repository.

**Rust — `fn main`.** `fn` declares a function. `main` takes no parameters
(empty parentheses) and returns nothing, so no `-> Type` is written. The body
is the block between braces.

**Rust — `println!`.** The `!` means this is a *macro*, not a function: code
that generates code at compile time. `println!` checks the format string at
compile time, which is why macros are used for formatting. Every statement ends
with `;`.

**Dart.** `main()` and `print()` play the same roles. The difference you will
feel first is that Rust has no runtime: this binary is a native executable
with no VM, and its start-up is the operating system calling `main`.

## 1.5 What the folder looks like now

```
time_tracker_study/
├── .gitignore
├── Cargo.toml
├── rustfmt.toml
├── guide/
│   ├── 00-outline.md
│   └── 01-workspace.md
└── crates/
    ├── core/
    │   ├── Cargo.toml
    │   └── src/lib.rs
    └── app/
        ├── Cargo.toml
        └── src/main.rs
```

The folder names `core` and `app` are *directories*; the package names inside
`Cargo.toml` (`work-time-core`, `work-time-tracker`) are what Cargo and Rust
code use. Both are allowed to differ.

## 1.6 Checkpoint

```sh
cargo run
```

Expected (the first run also prints `Compiling work-time-core` and
`Compiling work-time-tracker`):

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running `target/debug/work-time-tracker`
Work Time Tracker (study build)
```

```sh
cargo test --workspace
```

Expected: several blocks ending in `test result: ok. 0 passed; 0 failed; ...`.
No tests exist yet; what matters is that both crates compile as test targets.

Then the two checks you will run at the end of every chapter:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Both must print nothing except `Finished`. `-D warnings` turns every warning
into an error, so the build is either clean or it fails — there is no "I'll fix
those warnings later".

**Rust — `Cargo.lock`.** After the first build a `Cargo.lock` file appears.
It records the *exact* version of every dependency that was selected, so a
build tomorrow or on another machine picks the same versions. Applications
commit it; you should too. Never edit it by hand — Cargo rewrites it whenever
dependencies change. (Copying the original project's lock file is pointless at
this stage: Cargo would prune it to the two crates you are actually using.)

**Rust — the commands.**

| Command | Does |
| --- | --- |
| `cargo build` | compile (debug profile) into `target/debug/` |
| `cargo run` | build, then run the binary |
| `cargo test` | build the test targets and run every `#[test]` |
| `cargo fmt` | reformat all sources in place |
| `cargo clippy` | compile with extra lints |
| `-p work-time-core` | limit a command to one package |
| `--workspace` | apply to all packages |
| `--all-targets` | include tests, examples and benches |

Commit:

```sh
git add -A && git commit -m "Chapter 1: workspace"
```

## 1.7 Exercises

1. **Break the MSRV.** In the root `Cargo.toml` change `rust-version` to
   `"1.999"` and run `cargo build`. Read the error. Put it back.

   <details><summary>Answer</summary>

   Cargo stops before compiling anything:

   ```
   error: rustc 1.98.0 is not supported by the following packages:
     work-time-core@0.1.0 requires rustc 1.999
     work-time-tracker@0.1.0 requires rustc 1.999
   ```

   That is what the field is for: a clear message instead of a confusing
   syntax error from an old compiler.
   </details>

2. **Use the library from the binary.** Add this line to `lib.rs`:
   `pub const GREETING: &str = "core is linked";` and print it from `main.rs`
   with `println!("{}", work_time_core::GREETING);`. Run it. Then revert with
   `git checkout -- .`.

   <details><summary>Answer</summary>

   The binary sees the library through its package name with underscores,
   `work_time_core`. `pub` is needed: without it the constant is private to the
   library and the binary gets
   `error[E0603]: constant `GREETING` is private`. `&str` is a borrowed string
   slice; string literals have that type and live for the whole program.
   </details>

3. **Watch Clippy work.** In `main.rs`, add `let unused = 5;` inside `main`
   and run `cargo clippy --workspace -- -D warnings`. Then revert.

   <details><summary>Answer</summary>

   Clippy fails the build with `error: unused variable: `unused`` and suggests
   `_unused`. A leading underscore tells the compiler "I know this binding is
   unused". You will see `let _ignored = ...` in later chapters for values that
   are intentionally discarded.
   </details>

## Recap

- A workspace holds two packages: a pure library and an application binary.
  The dependency direction is one way: app → core.
- Versions live once, in the root catalog; crates opt in.
- `unsafe` is forbidden; `unwrap`/`expect` are denied so failures become
  values (`Result`) rather than crashes.
- `cargo run`, `cargo test --workspace`, `cargo fmt --all -- --check`, and
  `cargo clippy --workspace --all-targets -- -D warnings` are your checkpoint
  tools from now on.
