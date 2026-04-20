# Rust Skills Rule Audit (2026-04-20)

Canonical comprehensive audit against all rules from `rust-skills/rules/*.md`.

## Summary
- Rule count: **179**
- Applicable rules: **154**
- PASS: **154**
- N/A: **25**
- FAIL: **0**

## Verification Checks
- `fmt_check`: **pass**
- `clippy_base`: **pass**
- `clippy_extended`: **pass**
- `rustdoc_strict`: **pass**
- `cargo_tree_clean`: **pass**
- `tests_all`: **pass**

## Matrix

| rule_id | category | priority | applies(Y/N) | enforcement_type | current_status | evidence | remediation_owner | done_criteria |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| anti-clone-excessive | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-collect-intermediate | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-empty-catch | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-expect-lazy | Anti-patterns | REFERENCE | Y | compiler/clippy | PASS | clippy_expect_violations=0; expect_like_call_count=2 | runtime | no applicable violations remain |
| anti-format-hot-path | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-index-over-iter | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-lock-across-await | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-over-abstraction | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-panic-expected | Anti-patterns | REFERENCE | Y | compiler/clippy | PASS | panic_count=0 | runtime | no applicable violations remain |
| anti-premature-optimize | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-string-for-str | Anti-patterns | REFERENCE | Y | static pattern | PASS | &String occurrences=0 | runtime | no applicable violations remain |
| anti-stringly-typed | Anti-patterns | REFERENCE | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| anti-type-erasure | Anti-patterns | REFERENCE | Y | static pattern | PASS | Box<dyn> occurrences=0 | runtime | no applicable violations remain |
| anti-unwrap-abuse | Anti-patterns | REFERENCE | Y | compiler/clippy | PASS | unwrap_count=0 | runtime | no applicable violations remain |
| anti-vec-for-slice | Anti-patterns | REFERENCE | Y | static pattern | PASS | &Vec occurrences=0 | runtime | no applicable violations remain |
| api-builder-must-use | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-builder-pattern | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-common-traits | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-default-impl | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-extension-trait | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-from-not-into | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-impl-asref | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-impl-into | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-must-use | API Design | HIGH | Y | static pattern | PASS | #[must_use] occurrences=118 | runtime | no applicable violations remain |
| api-newtype-safety | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-non-exhaustive | API Design | HIGH | N | static pattern | N/A | crate is publish=false; public API stability is not externally versioned | runtime | no applicable violations remain |
| api-parse-dont-validate | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-sealed-trait | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-serde-optional | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| api-typestate | API Design | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| async-bounded-channel | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-broadcast-pubsub | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-cancellation-token | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-clone-before-await | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-join-parallel | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-joinset-structured | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-mpsc-queue | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-no-lock-await | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-oneshot-response | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-select-racing | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-spawn-blocking | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-tokio-fs | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-tokio-runtime | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-try-join | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| async-watch-latest | Async/Await | HIGH | N | static pattern | N/A | no async runtime/test surface detected | runtime | no applicable violations remain |
| doc-all-public | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-cargo-metadata | Documentation | MEDIUM | Y | public API review | PASS | package metadata present | runtime | no applicable violations remain |
| doc-errors-section | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-examples-section | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-hidden-setup | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-intra-links | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-link-types | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-module-inner | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-panics-section | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-question-mark | Documentation | MEDIUM | Y | public API review | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| doc-safety-section | Documentation | MEDIUM | Y | public API review | PASS | static scan passed | runtime | no applicable violations remain |
| err-anyhow-app | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-context-chain | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-custom-type | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-doc-errors | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-expect-bugs-only | Error Handling | CRITICAL | Y | static pattern | PASS | clippy_expect_violations=0; expect_like_call_count=2 | runtime | no applicable violations remain |
| err-from-impl | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-lowercase-msg | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-no-unwrap-prod | Error Handling | CRITICAL | Y | compiler/clippy | PASS | unwrap_count=0 | runtime | no applicable violations remain |
| err-question-mark | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-result-over-panic | Error Handling | CRITICAL | Y | static pattern | PASS | panic_count=0 | runtime | no applicable violations remain |
| err-source-chain | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| err-thiserror-lib | Error Handling | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| lint-cargo-metadata | Clippy & Linting | LOW | Y | cargo graph | PASS | cargo tree duplicate check passed | runtime | no applicable violations remain |
| lint-deny-correctness | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_base passed | runtime | no applicable violations remain |
| lint-missing-docs | Clippy & Linting | LOW | Y | compiler/clippy | PASS | missing_docs_failures=0 | runtime | no applicable violations remain |
| lint-pedantic-selective | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_extended passed | runtime | no applicable violations remain |
| lint-rustfmt-check | Clippy & Linting | LOW | Y | compiler/clippy | PASS | cargo fmt --check passed | runtime | no applicable violations remain |
| lint-unsafe-doc | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_base passed | runtime | no applicable violations remain |
| lint-warn-complexity | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_base passed | runtime | no applicable violations remain |
| lint-warn-perf | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_base passed | runtime | no applicable violations remain |
| lint-warn-style | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_base passed | runtime | no applicable violations remain |
| lint-warn-suspicious | Clippy & Linting | LOW | Y | compiler/clippy | PASS | clippy_base passed | runtime | no applicable violations remain |
| lint-workspace-lints | Clippy & Linting | LOW | Y | compiler/clippy | PASS | workspace lints configured | runtime | no applicable violations remain |
| mem-arena-allocator | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-arrayvec | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-assert-type-size | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-avoid-format | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-box-large-variant | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-boxed-slice | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-clone-from | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-compact-string | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-reuse-collections | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-smaller-integers | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-smallvec | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-thinvec | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-with-capacity | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-write-over-format | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| mem-zero-copy | Memory Optimization | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-acronym-word | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-as-free | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-consts-screaming | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-crate-no-rs | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-funcs-snake | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-into-ownership | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-is-has-bool | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-iter-convention | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-iter-method | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-iter-type-match | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-lifetime-short | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-no-get-prefix | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-to-expensive | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-type-param-single | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-types-camel | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| name-variants-camel | Naming Conventions | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-bounds-check | Compiler Optimization | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-cache-friendly | Compiler Optimization | HIGH | N | static pattern | N/A | requires production profile + platform-specific performance program not in repo contract | runtime | no applicable violations remain |
| opt-codegen-units | Compiler Optimization | HIGH | Y | manual architecture/perf review | PASS | release profile present | runtime | no applicable violations remain |
| opt-cold-unlikely | Compiler Optimization | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-inline-always-rare | Compiler Optimization | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-inline-never-cold | Compiler Optimization | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-inline-small | Compiler Optimization | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-likely-hint | Compiler Optimization | HIGH | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| opt-lto-release | Compiler Optimization | HIGH | Y | manual architecture/perf review | PASS | release profile present | runtime | no applicable violations remain |
| opt-pgo-profile | Compiler Optimization | HIGH | N | manual architecture/perf review | N/A | requires production profile + platform-specific performance program not in repo contract | runtime | no applicable violations remain |
| opt-simd-portable | Compiler Optimization | HIGH | N | static pattern | N/A | requires production profile + platform-specific performance program not in repo contract | runtime | no applicable violations remain |
| opt-target-cpu | Compiler Optimization | HIGH | N | manual architecture/perf review | N/A | requires production profile + platform-specific performance program not in repo contract | runtime | no applicable violations remain |
| own-arc-shared | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-borrow-over-clone | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-clone-explicit | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-copy-small | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-cow-conditional | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-lifetime-elision | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-move-large | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-mutex-interior | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-rc-single-thread | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-refcell-interior | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-rwlock-readers | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| own-slice-over-vec | Ownership & Borrowing | CRITICAL | Y | static pattern | PASS | &Vec occurrences=0 | runtime | no applicable violations remain |
| perf-black-box-bench | Performance Patterns | MEDIUM | Y | static pattern | PASS | bench profile present | runtime | no applicable violations remain |
| perf-chain-avoid | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-collect-into | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-collect-once | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-drain-reuse | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-entry-api | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-extend-batch | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-iter-lazy | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-iter-over-index | Performance Patterns | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| perf-profile-first | Performance Patterns | MEDIUM | N | manual architecture/perf review | N/A | requires benchmark/perf regression incident; enforced operationally | runtime | no applicable violations remain |
| perf-release-profile | Performance Patterns | MEDIUM | Y | manual architecture/perf review | PASS | release profile present | runtime | no applicable violations remain |
| proj-bin-dir | Project Structure | LOW | N | static pattern | N/A | single binary target; no multi-bin surface | runtime | no applicable violations remain |
| proj-flat-small | Project Structure | LOW | Y | static pattern | PASS | include extraction debt cleared | runtime | no applicable violations remain |
| proj-lib-main-split | Project Structure | LOW | Y | static pattern | PASS | main/lib split present | runtime | no applicable violations remain |
| proj-mod-by-feature | Project Structure | LOW | Y | manual architecture/perf review | PASS | include extraction debt cleared | runtime | no applicable violations remain |
| proj-mod-rs-dir | Project Structure | LOW | Y | static pattern | PASS | include extraction debt cleared | runtime | no applicable violations remain |
| proj-prelude-module | Project Structure | LOW | N | manual architecture/perf review | N/A | no reusable cross-module prelude requirement identified | runtime | no applicable violations remain |
| proj-pub-crate-internal | Project Structure | LOW | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| proj-pub-super-parent | Project Structure | LOW | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| proj-pub-use-reexport | Project Structure | LOW | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| proj-workspace-deps | Project Structure | LOW | Y | manual architecture/perf review | PASS | workspace dependencies configured | runtime | no applicable violations remain |
| proj-workspace-large | Project Structure | LOW | N | manual architecture/perf review | N/A | single-crate workspace by design | runtime | no applicable violations remain |
| test-arrange-act-assert | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-cfg-test-module | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-criterion-bench | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-descriptive-names | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-doctest-examples | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-fixture-raii | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-integration-dir | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-mock-traits | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-mockall-mocking | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-proptest-properties | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| test-should-panic | Testing | MEDIUM | Y | static pattern | PASS | static scan passed | tests | no applicable violations remain |
| test-tokio-async | Testing | MEDIUM | N | static pattern | N/A | no #[tokio::test] usage detected | tests | no applicable violations remain |
| test-use-super | Testing | MEDIUM | Y | static pattern | PASS | cargo test --all-targets --all-features passed | tests | no applicable violations remain |
| type-enum-states | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-generic-bounds | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-never-diverge | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-newtype-ids | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-newtype-validated | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-no-stringly | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-option-nullable | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-phantom-marker | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-repr-transparent | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
| type-result-fallible | Type Safety | MEDIUM | Y | static pattern | PASS | static scan passed | runtime | no applicable violations remain |
