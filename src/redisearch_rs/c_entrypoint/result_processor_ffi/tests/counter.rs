/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! This file contains tests to ensure the FFI functions behave as expected.

// Link both Rust-provided and C-provided symbols
extern crate redisearch_rs;
// Mock or stub the ones that aren't provided by the line above
redis_mock::mock_or_stub_missing_redis_c_symbols!();

use result_processor_ffi::counter::*;

#[test]
fn rp_counter_new_returns_valid_pointer() {
    // SAFETY: RPCounter_New returns a valid, initialized pointer.
    let counter = unsafe { RPCounter_New() };
    assert!(!counter.is_null(), "Should return non-null pointer");

    // SAFETY: counter is valid and was created by RPCounter_New.
    let free_fn = unsafe { (*counter) }
        .Free
        .expect("Rust result processor must have a free function");
    // SAFETY: counter is valid and free_fn expects this pointer.
    unsafe { free_fn(counter) };
}

#[test]
fn rp_counter_new_sets_correct_type() {
    // SAFETY: RPCounter_New returns a valid, initialized pointer.
    let counter = unsafe { RPCounter_New() };

    // SAFETY: counter is valid.
    assert_eq!(
        unsafe { (*counter).type_ },
        ffi::ResultProcessorType_RP_COUNTER,
        "Counter should set type `ffi::ResultProcessorType_RP_COUNTER`"
    );

    // SAFETY: counter is valid and was created by RPCounter_New.
    let free_fn = unsafe { (*counter) }
        .Free
        .expect("Rust result processor must have a free function");
    // SAFETY: counter is valid and free_fn expects this pointer.
    unsafe { free_fn(counter) };
}

#[test]
fn rp_counter_new_creates_unique_instances() {
    // SAFETY: RPCounter_New returns a valid, initialized pointer.
    let counter1 = unsafe { RPCounter_New() };
    // SAFETY: RPCounter_New returns a valid, initialized pointer.
    let counter2 = unsafe { RPCounter_New() };

    assert_ne!(counter1, counter2, "Should create unique instances");

    // SAFETY: counter1 is valid and was created by RPCounter_New.
    let free_fn1 = unsafe { (*counter1) }
        .Free
        .expect("Rust result processor must have a free function");
    // SAFETY: counter1 is valid and free_fn1 expects this pointer.
    unsafe { free_fn1(counter1) };

    // SAFETY: counter2 is valid and was created by RPCounter_New.
    let free_fn2 = unsafe { (*counter2) }
        .Free
        .expect("Rust result processor must have a free function");
    // SAFETY: counter2 is valid and free_fn2 expects this pointer.
    unsafe { free_fn2(counter2) };
}
