/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::{c_char, c_int};
use std::slice;

/// Opaque type BloomFilter. Can be instantiated with [`BloomFilter_New`].
pub struct BloomFilter(bloom_filter::BloomFilter);

/// Create a new [`BloomFilter`]. Returns an opaque pointer to the newly created filter.
///
/// # Arguments
///
/// * `expected_items` - Expected number of unique items to be inserted
/// * `false_positive_rate` - Desired false positive rate (e.g., 0.01 for 1%)
///
/// To free the filter, use [`BloomFilter_Free`].
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `expected_items` must be greater than 0
/// - `false_positive_rate` must be between 0.0 and 1.0 (exclusive)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BloomFilter_New(
    expected_items: usize,
    false_positive_rate: f64,
) -> *mut BloomFilter {
    let filter = Box::new(BloomFilter(bloom_filter::BloomFilter::new(
        expected_items,
        false_positive_rate,
    )));
    Box::into_raw(filter)
}

/// Insert an item into the Bloom filter.
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `filter` must point to a valid BloomFilter obtained from [`BloomFilter_New`] and cannot be NULL.
/// - `item` can be NULL only if `len == 0`. It is not necessarily NULL-terminated.
/// - `len` can be 0. If so, `item` is regarded as an empty string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BloomFilter_Insert(
    filter: *mut BloomFilter,
    item: *const c_char,
    len: usize,
) {
    debug_assert!(!filter.is_null(), "filter cannot be NULL");

    // SAFETY: The safety requirements of this function
    // require the caller to ensure that the pointer `filter` is
    // a valid BloomFilter obtained from `BloomFilter_New` and cannot be NULL.
    // If that invariant is upheld, then the following line is sound.
    let BloomFilter(bloom) = unsafe { &mut *filter };

    let key: &[u8] = if len > 0 {
        debug_assert!(!item.is_null(), "item cannot be NULL if len > 0");
        // SAFETY: The safety requirements of this function
        // require the caller to ensure that the pointer `item` is
        // a valid pointer to a C string, with a length of `len` bytes.
        // If that invariant is upheld, then the following line is sound.
        unsafe { slice::from_raw_parts(item.cast(), len) }
    } else {
        &[]
    };

    bloom.insert(key);
}

/// Check if an item might be in the Bloom filter.
///
/// Returns 1 if the item might be in the set (with possible false positives),
/// or 0 if the item is definitely not in the set.
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `filter` must point to a valid BloomFilter obtained from [`BloomFilter_New`] and cannot be NULL.
/// - `item` can be NULL only if `len == 0`. It is not necessarily NULL-terminated.
/// - `len` can be 0. If so, `item` is regarded as an empty string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BloomFilter_Contains(
    filter: *const BloomFilter,
    item: *const c_char,
    len: usize,
) -> c_int {
    debug_assert!(!filter.is_null(), "filter cannot be NULL");

    // SAFETY: The safety requirements of this function
    // require the caller to ensure that the pointer `filter` is
    // a valid BloomFilter obtained from `BloomFilter_New` and cannot be NULL.
    // If that invariant is upheld, then the following line is sound.
    let BloomFilter(bloom) = unsafe { &*filter };

    let key: &[u8] = if len > 0 {
        debug_assert!(!item.is_null(), "item cannot be NULL if len > 0");
        // SAFETY: The safety requirements of this function
        // require the caller to ensure that the pointer `item` is
        // a valid pointer to a C string, with a length of `len` bytes.
        // If that invariant is upheld, then the following line is sound.
        unsafe { slice::from_raw_parts(item.cast(), len) }
    } else {
        &[]
    };

    if bloom.contains(key) {
        1
    } else {
        0
    }
}

/// Get the number of items inserted into the filter.
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `filter` must point to a valid BloomFilter obtained from [`BloomFilter_New`] and cannot be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BloomFilter_Count(filter: *const BloomFilter) -> usize {
    debug_assert!(!filter.is_null(), "filter cannot be NULL");

    // SAFETY: The safety requirements of this function
    // require the caller to ensure that the pointer `filter` is
    // a valid BloomFilter obtained from `BloomFilter_New` and cannot be NULL.
    // If that invariant is upheld, then the following line is sound.
    let BloomFilter(bloom) = unsafe { &*filter };
    bloom.count()
}

/// Get the memory usage of the filter in bytes.
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `filter` must point to a valid BloomFilter obtained from [`BloomFilter_New`] and cannot be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BloomFilter_MemUsage(filter: *const BloomFilter) -> usize {
    debug_assert!(!filter.is_null(), "filter cannot be NULL");

    // SAFETY: The safety requirements of this function
    // require the caller to ensure that the pointer `filter` is
    // a valid BloomFilter obtained from `BloomFilter_New` and cannot be NULL.
    // If that invariant is upheld, then the following line is sound.
    let BloomFilter(bloom) = unsafe { &*filter };
    bloom.mem_usage()
}

/// Free the Bloom filter.
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `filter` must point to a valid BloomFilter obtained from [`BloomFilter_New`].
/// - `filter` can be NULL, in which case this function does nothing.
/// - After calling this function, `filter` must not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BloomFilter_Free(filter: *mut BloomFilter) {
    if filter.is_null() {
        return;
    }

    // Reconstruct the original Box<BloomFilter> which will take care of freeing the memory
    // upon dropping.
    // SAFETY: The safety requirements of this function
    // state the caller is to ensure that the pointer `filter` is
    // a valid BloomFilter obtained from `BloomFilter_New`.
    // If that invariant is upheld, then the following line is sound.
    let _filter = unsafe { Box::from_raw(filter) };
}

