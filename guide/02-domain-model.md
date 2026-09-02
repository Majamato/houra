# Chapter 2 — Domain model

**Goal.** The core crate gets its first real types: three ID types, `Project`,
`Task`, `TimeEntry`, the `EntrySource` enum, validation functions, and the
`DomainError` type every rule violation is reported with. Files:
`crates/core/src/lib.rs`, `crates/core/src/model.rs`, `crates/core/src/error.rs`.

**You will learn**

- Structs, enums, `impl` blocks, and the standard traits you get from `#[derive]`.
- Ownership at its most basic: owned `String` versus borrowed `&str`, and `&self`.
- `Option` and `Result`, and the `?` operator.
- How modules map to files, what `pub` means, and re-exporting with `pub use`.
- A declarative macro (`macro_rules!`) and why a *newtype* is worth it.
- Typed errors with `thiserror`.

**Prerequisite.** Chapter 1 checkpoint passed.

---

## 2.1 Modules and ID types

First, tell the core crate it needs two libraries:

```toml
# crates/core/Cargo.toml
[dependencies]
serde.workspace = true
thiserror.workspace = true
# ...
```

**What.** `serde` converts Rust values to and from formats like JSON (the
tracker state is stored as JSON inside SQLite from Chapter 9). `thiserror`
writes the boring parts of error types for you.

Replace `lib.rs`:

```rust
// crates/core/src/lib.rs
//! Domain rules for Houra.
//!
//! This crate knows nothing about GTK, SQLite, D-Bus, or the filesystem. That
//! boundary keeps the timer rules deterministic and cheap to test.

mod model;

pub use model::*;
```

**Rust — modules.** `mod model;` tells the compiler: "there is a module named
`model`; load it from `src/model.rs`". Modules are Rust's namespaces, and every
file is a module. Without `pub`, `mod model` is private: nobody outside this
crate can write `houra_core::model::Project`.

**Rust — re-exports.** `pub use model::*;` takes every public item from
`model` and re-exports it at the crate root, so users write
`houra_core::Project`. The `*` is a glob import. The file layout stays an
internal detail; you could later split `model.rs` into three files without
changing any caller.

**Idiom.** Keep modules private and re-export a deliberate public surface from
`lib.rs`. What you re-export is your API; the file tree is not.

Now the first part of `model.rs`:

```rust
// crates/core/src/model.rs
use serde::{Deserialize, Serialize};

/// Defines a distinct integer ID type so IDs of different entities cannot be
/// mixed up by accident.
macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            Deserialize,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl $name {
            pub const fn new(value: i64) -> Self {
                Self(value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

id_type!(ProjectId);
id_type!(TaskId);
id_type!(EntryId);
```

**What.** Three types — `ProjectId`, `TaskId`, `EntryId` — each wrapping one
`i64` (a 64-bit signed integer, which is what SQLite uses for row IDs).

**Why.** A database ID is "just a number", and that is the problem: a function
taking `(i64, i64)` lets you pass a task ID where a project ID belongs, and
nothing complains until data is corrupted. With three distinct types the
compiler rejects the mix-up (Exercise 1 shows the message). This is the
*newtype* pattern: a zero-cost wrapper whose only purpose is to be a different
type.

**Rust — `use`.** `use serde::{Deserialize, Serialize};` imports two names so
the code can say `Serialize` instead of `serde::Serialize`. `::` walks into a
crate, a module, or a type. Braces import several names from one path.

**Rust — tuple structs.** `pub struct ProjectId(pub i64);` declares a struct
with one unnamed field, accessed as `.0`. Construct it with `ProjectId(7)`.
The `pub` on the field allows other modules to read and construct it.

**Rust — `#[derive(...)]`.** An *attribute* (`#[...]`) annotates the item
below it. `derive` asks the compiler to write standard trait implementations
for the type. A *trait* is a set of behaviours a type can implement — think
interface. The ones derived here:

| Trait | Gives you |
| --- | --- |
| `Clone` | `.clone()` — an explicit copy |
| `Copy` | the value is copied implicitly on assignment (only for small, plain data) |
| `Debug` | `{:?}` formatting, e.g. `EntryId(42)` — for logs and test failures |
| `Default` | `ProjectId::default()`, here `ProjectId(0)` |
| `PartialEq`, `Eq` | `==` and `!=` |
| `Hash` | usable as a key in hash maps |
| `PartialOrd`, `Ord` | `<`, `>`, sorting |
| `Serialize`, `Deserialize` | serde conversion (these two come from serde, not the standard library) |

`#[serde(transparent)]` makes serde treat `ProjectId(7)` as the bare number
`7` in JSON, so the wrapper never leaks into files.

**Rust — `impl` blocks.** `impl ProjectId { ... }` attaches functions to the
type. `new` has no `self` parameter, so it is an *associated function* called
as `ProjectId::new(7)`. Inside an `impl`, `Self` is shorthand for the type.
`const fn` means the function may also run at compile time (for example to
initialise a constant); it changes nothing about how you call it.

**Rust — implementing a trait by hand.** `impl std::fmt::Display for
ProjectId` provides the `Display` trait, which is what `{}` in `format!` and
`println!` uses. The trait has one required method, `fmt`. We delegate to the
inner `i64`'s own `fmt`, so a project ID prints as `7`, not `ProjectId(7)`.
`&mut` is an exclusive, writable borrow — the formatter is being written to.
The `'_` in `Formatter<'_>` is a *lifetime* placeholder; ignore it for now, it
just means "borrowed for as long as this call".

**Rust — `macro_rules!`.** Three types need the same forty lines. Rust has no
inheritance and no templates in the C++ sense; instead a *declarative macro*
pastes a pattern. `($name:ident) => { ... }` says: the macro takes one
identifier, and everywhere `$name` appears the identifier is substituted.
`id_type!(ProjectId);` expands to the whole block with `ProjectId` filled in.
Macros are how Rust removes repetition that generics cannot.

**Dart.** Dart has no equivalent to the newtype pattern with zero runtime
cost; you would use `extension type` (Dart 3.3+) for the same idea.

Build now, before going on:

```sh
cargo build -p houra-core
```

It should finish with no warnings. (A "unused" warning would mean you forgot
`pub` somewhere; everything in this file is public, so nothing is unused.)

## 2.2 The domain error

Every rule this crate enforces produces a value of one type. Create it:

```rust
// crates/core/src/error.rs
use thiserror::Error;

use crate::{EntryId, ProjectId, TaskId};

/// A rejected domain operation. Infrastructure errors belong in the app crate.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("end time {end_ms} must be after start time {start_ms}")]
    InvalidInterval { start_ms: i64, end_ms: i64 },
    #[error(
        "idle start {idle_start_ms} is outside the active interval beginning {active_start_ms}"
    )]
    InvalidIdleStart {
        active_start_ms: i64,
        idle_start_ms: i64,
    },
    #[error("return time {return_ms} is before idle start {idle_start_ms}")]
    InvalidReturn { idle_start_ms: i64, return_ms: i64 },
    #[error("task {task_id} does not belong to project {project_id}")]
    TaskProjectMismatch {
        project_id: ProjectId,
        task_id: TaskId,
    },
    #[error("entry overlaps existing entries: {conflicts:?}")]
    Overlap { conflicts: Vec<EntryId> },
    #[error("name must contain a non-whitespace character")]
    EmptyName,
    #[error("color must be a CSS hexadecimal color such as #3584e4")]
    InvalidColor,
    #[error("recovery end {end_ms} is outside [{start_ms}, {last_heartbeat_ms}]")]
    InvalidRecoveryEnd {
        start_ms: i64,
        last_heartbeat_ms: i64,
        end_ms: i64,
    },
}
```

And register the module:

```rust
// crates/core/src/lib.rs
// ...
mod error;
mod model;

pub use error::DomainError;
pub use model::*;
```

**What.** `DomainError` lists every way a domain operation can be rejected.
Some variants carry data (`InvalidInterval` has the two timestamps); some carry
none (`EmptyName`). Chapter 3 adds one more variant, `InvalidState`, once the
tracker state type exists.

**Why.** The GTK window will show these messages; the storage layer will
refuse to write when it gets one. Because the error is a *type* with variants,
callers can `match` on the exact case instead of parsing message strings.

**Rust — enums.** An `enum` is a closed set of alternatives. Unlike Dart
enums, each variant can carry its own fields. A variant written with braces
(`InvalidInterval { start_ms: i64, end_ms: i64 }`) is *struct-like*: it is
constructed and matched by field name, so two `i64`s cannot be confused.
`Vec<EntryId>` is an owned, growable list — Rust's `List<T>`.

**Rust — `crate::`.** A path starting with `crate::` begins at the crate root
(`lib.rs`). Since `lib.rs` re-exports the ID types, `crate::ProjectId` works
from any module.

**Rust — `thiserror`.** The standard library has an `Error` trait and a
`Display` trait; writing both by hand for nine variants is tedious.
`#[derive(Error)]` plus one `#[error("...")]` per variant generates them. Inside
the format string, `{end_ms}` prints a field with `Display` and
`{conflicts:?}` prints one with `Debug` (a `Vec` has no `Display`). Notice that
`{task_id}` works because we implemented `Display` for the ID types in 2.1.

**Idiom.** One error enum per layer. The core has `DomainError` (rule
violations); the app crate will have `AppError` (database, file, thread
failures) that *contains* a `DomainError`. Errors describe what went wrong in
the vocabulary of their layer.

```sh
cargo build -p houra-core
```

## 2.3 Projects, tasks, and time entries

Append to `model.rs`, and add the `DomainError` import at the top:

```rust
// crates/core/src/model.rs
use serde::{Deserialize, Serialize};

use crate::DomainError;

// ... (id_type! and the three IDs from 2.1)

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub color: String,
    pub archived: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Project {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_name(&self.name)?;
        validate_color(&self.color)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub name: String,
    pub archived: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl Task {
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_name(&self.name)
    }
}
```

**What.** Two ordinary structs with named fields, each with a `validate`
method. Timestamps are `i64` milliseconds since 1970 (UTC). `archived` exists
because a project referenced by history can never be deleted — it is hidden
instead (Chapter 10 enforces this in SQL).

**Rust — `String` versus `&str`.** `name: String` means the struct *owns* its
text: when the `Project` is dropped, the text's memory is freed with it.
Because `String` owns heap memory, it is not `Copy`: assigning a `Project` to
another variable *moves* it, and the old variable can no longer be used
(Exercise 2). `Clone` is derived so callers can ask for a copy explicitly.
`bool` and `i64` are `Copy`; they are duplicated silently.

**Rust — `&self`.** A method's first parameter decides how it accesses the
value: `&self` borrows it read-only, `&mut self` borrows it for writing, and
plain `self` takes ownership. `validate` only reads, so `&self`. The `&` is
the *borrow* operator: `&self.name` lends the `String` as a `&str` without
copying or moving it.

**Rust — `Result` and `?`.** `Result<(), DomainError>` means: on success there
is no payload (`()` is the empty tuple, Rust's "nothing"), on failure there is a
`DomainError`. The `?` after `validate_name(&self.name)` means "if this
returned `Err`, return that `Err` from *this* function right now; otherwise
continue with the `Ok` value". The last line has no `;`, so its value
(the `Result` from `validate_color`) is the function's return value.

**Idiom.** Take `&str` as a parameter (works with `String`, literals, slices);
store `String` in structs (owned data has a clear lifetime). Return `Result`,
never panic, from library code.

**Dart.** `Result` replaces exceptions. There is no `try`/`catch` in Rust; an
error is a value you either handle or pass up with `?`.

Now the enum and the entry:

```rust
// crates/core/src/model.rs
// ...

/// How a time entry came into existence.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    #[default]
    Timer,
    Manual,
    IdleReassignment,
    Recovery,
}

/// A completed, half-open interval `[start_ms, end_ms)` of tracked time.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TimeEntry {
    /// `None` until SQLite assigns an ID on insert.
    pub id: Option<EntryId>,
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
    pub note: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub source: EntrySource,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl TimeEntry {
    pub fn duration_ms(&self) -> i64 {
        self.end_ms.saturating_sub(self.start_ms).max(0)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.end_ms <= self.start_ms {
            return Err(DomainError::InvalidInterval {
                start_ms: self.start_ms,
                end_ms: self.end_ms,
            });
        }
        Ok(())
    }
}
```

**What.** `EntrySource` records where an entry came from: the timer, a manual
form, the "reassign idle time" dialog, or crash recovery. `TimeEntry` is the
central record of the application.

**Why half-open.** `[start_ms, end_ms)` includes the start and excludes the
end. Two entries `[0, 10)` and `[10, 20)` touch without overlapping, so a day
of back-to-back work has no gaps and no double counting. Chapter 7 and the SQL
triggers in Chapter 9 rely on this rule.

**Rust — `Option<T>`.** Rust has no `null`. A value that may be absent is an
`Option<T>`: either `Some(value)` or `None`. A new entry has no database ID
yet, so `id: Option<EntryId>`; an entry without a task has `task_id: None`.
The compiler forces every reader to handle the `None` case.

**Rust — `#[default]`.** With `Default` derived on an enum, `#[default]` marks
which variant `EntrySource::default()` returns. `#[serde(rename_all =
"snake_case")]` stores `IdleReassignment` as `"idle_reassignment"` in JSON.
The enum has no data, so it is `Copy`.

**Rust — early `return` and expressions.** Inside `validate`, `return
Err(...)` leaves the function immediately. `Ok(())` at the end has no `;`, so
it is the value of the function. `if` is an expression too: it can produce a
value, which `validate_name` below uses.

**Rust — saturating arithmetic.** `end_ms - start_ms` on `i64` *panics in
debug builds* if it overflows. `saturating_sub` clamps to the minimum instead,
and `.max(0)` guarantees a non-negative display value even for an invalid
entry. `duration_ms` is a display helper; `validate` still rejects the bad
interval before it reaches the database.

Finally, the two validation functions:

```rust
// crates/core/src/model.rs
// ...

pub fn validate_name(name: &str) -> Result<(), DomainError> {
    if name.trim().is_empty() {
        Err(DomainError::EmptyName)
    } else {
        Ok(())
    }
}

pub fn validate_color(color: &str) -> Result<(), DomainError> {
    let valid = color.len() == 7
        && color.starts_with('#')
        && color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(DomainError::InvalidColor)
    }
}
```

**What.** Free functions (not attached to a type) shared by `Project`, `Task`,
and later by the storage layer, which validates user input before writing.

**Rust — `&str`.** A `&str` is a borrowed view of UTF-8 text. Accepting `&str`
lets callers pass a `String` (with `&`), a literal, or a slice. `trim()`
returns another `&str` pointing into the same text — no allocation.

**Rust — `let` and `if` as expression.** `let valid = ...;` binds an immutable
name. Both branches of the `if`/`else` produce a `Result`, and the whole `if`
is the function's return value.

**Rust — iterators and closures.** `color.bytes()` produces the bytes one by
one; `.skip(1)` drops the `#`; `.all(|byte| ...)` returns `true` if the
closure is true for every byte. `|byte| byte.is_ascii_hexdigit()` is a
*closure*: an anonymous function whose parameters go between `|` bars.
Iterator chains are lazy — `all` stops at the first non-hex byte — and compile
to the same code as a hand-written loop. `&&` short-circuits, so `bytes()` is
not even called for a string of the wrong length.

**Idiom.** Prefer iterator chains to index loops; they cannot go out of
bounds and they say what you mean (`all`, `any`, `map`, `filter`).

## 2.4 Checkpoint

```sh
cargo build -p houra-core
cargo clippy -p houra-core --all-targets -- -D warnings
cargo fmt --all -- --check
```

All three: only `Finished` (or nothing) — no warnings. Then:

```sh
diff <(grep -v '^\s*//' ../work_time_tracker/crates/core/src/model.rs) \
     <(grep -v '^\s*//' crates/core/src/model.rs)
```

You should see only the types that Chapter 3 adds (`ActiveTimer`,
`TrackerState`, `TrackerCommand`, …) as missing on your side, plus doc-comment
differences. Everything you typed should match.

```sh
git add -A && git commit -m "Chapter 2: domain model"
```

## 2.5 Exercises

Do each one, read the output, then `git checkout -- .`.

1. **The compiler catches ID mix-ups.** Append this to `model.rs`:

   ```rust
   #[cfg(test)]
   mod mixup {
       use super::*;

       #[test]
       fn wrong_id() {
           let task_id = TaskId(7);
           let project = Project {
               id: task_id,
               name: "x".into(),
               color: "#000000".into(),
               archived: false,
               created_at_ms: 0,
               updated_at_ms: 0,
           };
           let _ = project;
       }
   }
   ```

   Run `cargo test -p houra-core`.

   <details><summary>Answer</summary>

   ```
   error[E0308]: mismatched types
      --> crates/core/src/model.rs:146:17
       |
   146 |             id: task_id,
       |                 ^^^^^^^ expected `ProjectId`, found `TaskId`
   ```

   Two wrappers around the same `i64` are different types. This is the whole
   payoff of `id_type!`. Also new here: `#[cfg(test)]` compiles the module only
   for `cargo test`; `use super::*;` imports everything from the parent module;
   `"x".into()` converts a `&str` literal into the `String` the field needs;
   `let _ = project;` discards a value on purpose.
   </details>

2. **A move is not a copy.** Replace the module with:

   ```rust
   #[cfg(test)]
   mod moves {
       use super::*;

       #[test]
       fn moved_name() {
           let name = String::from("Design");
           let project = Project {
               id: ProjectId(1),
               name,
               color: "#000000".into(),
               archived: false,
               created_at_ms: 0,
               updated_at_ms: 0,
           };
           println!("{name} {}", project.name);
       }
   }
   ```

   <details><summary>Answer</summary>

   ```
   error[E0382]: borrow of moved value: `name`
       |
   144 |         let name = String::from("Design");
       |             ---- move occurs because `name` has type `String`, which does not implement the `Copy` trait
   147 |             name,
       |             ---- value moved here
   153 |         println!("{name} {}", project.name);
       |                    ^^^^ value borrowed here after move
       |
   help: consider cloning the value if the performance cost is acceptable
       |
   147 |             name: name.clone(),
   ```

   `name,` is the field-init shorthand for `name: name`, and it *moves* the
   `String` into the struct. After that the old binding is dead. The compiler
   tells you exactly where the move happened and offers `.clone()`. In Rust
   every value has exactly one owner at a time; that rule is what makes memory
   safety possible without a garbage collector.
   </details>

3. **Display versus Debug.** Replace the module with:

   ```rust
   #[cfg(test)]
   mod display {
       use super::*;

       #[test]
       fn show() {
           let id = EntryId(42);
           println!("display={id} debug={id:?}");
           let err = DomainError::Overlap { conflicts: vec![EntryId(1), EntryId(2)] };
           println!("{err}");
           panic!("show output");
       }
   }
   ```

   Run `cargo test -p houra-core show`.

   <details><summary>Answer</summary>

   ```
   display=42 debug=EntryId(42)
   entry overlaps existing entries: [EntryId(1), EntryId(2)]
   ```

   `{id}` uses your `Display` impl, `{id:?}` uses the derived `Debug`. The
   error message comes from the `#[error(...)]` attribute; `{conflicts:?}`
   formats the `Vec` with `Debug`. The `panic!` is only there because
   `cargo test` hides the output of passing tests; a failing test shows it.
   `vec![...]` is the macro that builds a `Vec` from a list.
   </details>

4. **Remove a `?`.** In `Project::validate`, delete the `?` after
   `validate_name(&self.name)` and run `cargo build -p houra-core`.

   <details><summary>Answer</summary>

   ```
   warning: unused `Result` that must be used
     --> crates/core/src/model.rs:55:9
      |
   55 |         validate_name(&self.name);
   ```

   `Result` is marked `#[must_use]`: silently dropping an error is almost
   always a bug, so the compiler warns — and with `-D warnings` your checkpoint
   fails. The `?` is what turns "a result was produced" into "a failure stops
   this function".
   </details>

5. **Write real unit tests (keep this one).** Append:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn blank_names_are_rejected() {
           assert_eq!(validate_name("   "), Err(DomainError::EmptyName));
           assert!(validate_name("Design").is_ok());
       }

       #[test]
       fn colors_must_be_seven_char_hex() {
           assert!(validate_color("#3584e4").is_ok());
           assert_eq!(validate_color("3584e4"), Err(DomainError::InvalidColor));
           assert_eq!(validate_color("#35g4e4"), Err(DomainError::InvalidColor));
       }

       #[test]
       fn duration_never_goes_negative() {
           let entry = TimeEntry {
               id: None,
               project_id: ProjectId(1),
               task_id: None,
               note: String::new(),
               start_ms: 200,
               end_ms: 100,
               source: EntrySource::Manual,
               created_at_ms: 0,
               updated_at_ms: 0,
           };
           assert_eq!(entry.duration_ms(), 0);
           assert!(entry.validate().is_err());
       }
   }
   ```

   Run `cargo test -p houra-core`. Expected:

   ```
   test model::tests::blank_names_are_rejected ... ok
   test model::tests::colors_must_be_seven_char_hex ... ok
   test model::tests::duration_never_goes_negative ... ok
   test result: ok. 3 passed; 0 failed; ...
   ```

   The original project has no unit tests in `model.rs` (its tests live in
   `tests/`, from Chapter 5). Keeping these three is a small, deliberate
   difference: unit tests next to the code they check are normal Rust practice.
   Decide now — keep them, or `git checkout -- .` — and be consistent later.

   <details><summary>Answer</summary>

   `assert_eq!(a, b)` compares with `==` (both sides need `PartialEq`, which is
   why `DomainError` derives it) and prints both values with `Debug` on
   failure. `assert!(cond)` checks a boolean. `is_ok()` / `is_err()` test a
   `Result` without looking inside. `String::new()` is an empty owned string.
   </details>

## Recap

- Three newtype IDs make the compiler catch mixed-up numbers; a macro writes
  them once.
- `Project`, `Task` and `TimeEntry` own their strings; validation borrows
  (`&self`, `&str`) and returns `Result<(), DomainError>`.
- `?` propagates errors; `Option` replaces null; `Result` replaces exceptions.
- Modules are files; `lib.rs` chooses what becomes public with `pub use`.
- Standard behaviours (`Clone`, `Debug`, `PartialEq`, …) come from
  `#[derive]`; `Display` and `Error` were implemented by hand and by
  `thiserror`.

Next: **Chapter 3 — Tracker state**, where an enum with data makes it
impossible for the timer to be "stopped and idle" at the same time.
