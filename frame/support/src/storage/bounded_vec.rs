// This file is part of Substrate.

// Copyright (C) 2017-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Traits, types and structs to support putting a bounded vector into storage, as a raw value, map
//! or a double map.

use sp_std::prelude::*;
use sp_std::{convert::TryFrom, fmt, marker::PhantomData};
use codec::{FullCodec, Encode, EncodeLike, Decode};
use core::{
	ops::{Deref, Index, IndexMut},
	slice::SliceIndex,
};
use crate::{
	traits::{Get, MaxEncodedLen},
	storage::{generator, StorageDecodeLength, StorageValue, StorageMap, StorageDoubleMap},
};

/// A bounded vector.
///
/// It has implementations for efficient append and length decoding, as with a normal `Vec<_>`, once
/// put into storage as a raw value, map or double-map.
///
/// As the name suggests, the length of the queue is always bounded. All internal operations ensure
/// this bound is respected.
#[derive(Encode, Decode)]
pub struct BoundedVec<T, S>(Vec<T>, PhantomData<S>);

impl<T, S> BoundedVec<T, S> {
	/// Create `Self` from `t` without any checks.
	unsafe fn unchecked_from(t: Vec<T>) -> Self {
		Self(t, Default::default())
	}

	/// Consume self, and return the inner `Vec`. Henceforth, the `Vec<_>` can be altered in an
	/// arbitrary way. At some point, if the reverse conversion is required, `TryFrom<Vec<_>>` can
	/// be used.
	///
	/// This is useful for cases if you need access to an internal API of the inner `Vec<_>` which
	/// is not provided by the wrapper `BoundedVec`.
	pub fn into_inner(self) -> Vec<T> {
		self.0
	}

	/// Exactly the same semantics as [`Vec::remove`].
	///
	/// # Panics
	///
	/// Panics if `index` is out of bounds.
	pub fn remove(&mut self, index: usize) {
		self.0.remove(index);
	}

	/// Exactly the same semantics as [`Vec::swap_remove`].
	///
	/// # Panics
	///
	/// Panics if `index` is out of bounds.
	pub fn swap_remove(&mut self, index: usize) {
		self.0.swap_remove(index);
	}

	/// Exactly the same semantics as [`Vec::retain`].
	pub fn retain<F: FnMut(&T) -> bool>(&mut self, f: F) {
		self.0.retain(f)
	}
}

impl<T, S: Get<u32>> BoundedVec<T, S> {
	/// Get the bound of the type in `usize`.
	pub fn bound() -> usize {
		S::get() as usize
	}

	/// Create `Self` from `t` without any checks. Logs warnings if the bound is not being
	/// respected. The additional scope can be used to indicate where a potential overflow is
	/// happening.
	pub unsafe fn force_from(t: Vec<T>, scope: Option<&'static str>) -> Self {
		if t.len() > Self::bound() {
			log::warn!(
				target: crate::LOG_TARGET,
				"length of a bounded vector in scope {} is not respected.",
				scope.unwrap_or("UNKNOWN"),
			);
		}

		Self::unchecked_from(t)
	}

	/// Consumes self and mutates self via the given `mutate` function.
	///
	/// If the outcome of mutation is within bounds, `Some(Self)` is returned. Else, `None` is
	/// returned.
	///
	/// This is essentially a *consuming* shorthand [`Self::into_inner`] -> `...` ->
	/// [`Self::try_from`].
	pub fn try_mutate(mut self, mut mutate: impl FnMut(&mut Vec<T>)) -> Option<Self> {
		mutate(&mut self.0);
		(self.0.len() <= Self::bound()).then(move || self)
	}

	/// Exactly the same semantics as [`Vec::insert`], but returns an `Err` (and is a noop) if the
	/// new length of the vector exceeds `S`.
	///
	/// # Panics
	///
	/// Panics if `index > len`.
	pub fn try_insert(&mut self, index: usize, element: T) -> Result<(), ()> {
		if self.len() < Self::bound() {
			self.0.insert(index, element);
			Ok(())
		} else {
			Err(())
		}
	}

	/// Exactly the same semantics as [`Vec::push`], but returns an `Err` (and is a noop) if the
	/// new length of the vector exceeds `S`.
	///
	/// # Panics
	///
	/// Panics if the new capacity exceeds isize::MAX bytes.
	pub fn try_push(&mut self, element: T) -> Result<(), ()> {
		if self.len() < Self::bound() {
			self.0.push(element);
			Ok(())
		} else {
			Err(())
		}
	}
}

impl<T, S> Default for BoundedVec<T, S> {
	fn default() -> Self {
		// the bound cannot be below 0, which is satisfied by an empty vector
		unsafe { Self::unchecked_from(Vec::default()) }
	}
}

#[cfg(feature = "std")]
impl<T, S> fmt::Debug for BoundedVec<T, S>
where
	T: fmt::Debug,
	S: Get<u32>,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_tuple("BoundedVec").field(&self.0).field(&Self::bound()).finish()
	}
}

impl<T, S> Clone for BoundedVec<T, S>
where
	T: Clone,
{
	fn clone(&self) -> Self {
		// bound is retained
		unsafe { Self::unchecked_from(self.0.clone()) }
	}
}

impl<T, S: Get<u32>> TryFrom<Vec<T>> for BoundedVec<T, S> {
	type Error = ();
	fn try_from(t: Vec<T>) -> Result<Self, Self::Error> {
		if t.len() <= Self::bound() {
			// explicit check just above
			Ok(unsafe { Self::unchecked_from(t) })
		} else {
			Err(())
		}
	}
}

// It is okay to give a non-mutable reference of the inner vec to anyone.
impl<T, S> AsRef<Vec<T>> for BoundedVec<T, S> {
	fn as_ref(&self) -> &Vec<T> {
		&self.0
	}
}

impl<T, S> AsRef<[T]> for BoundedVec<T, S> {
	fn as_ref(&self) -> &[T] {
		&self.0
	}
}

impl<T, S> AsMut<[T]> for BoundedVec<T, S> {
	fn as_mut(&mut self) -> &mut [T] {
		&mut self.0
	}
}

// will allow for immutable all operations of `Vec<T>` on `BoundedVec<T>`.
impl<T, S> Deref for BoundedVec<T, S> {
	type Target = Vec<T>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

// Allows for indexing similar to a normal `Vec`. Can panic if out of bound.
impl<T, S, I> Index<I> for BoundedVec<T, S>
where
	I: SliceIndex<[T]>,
{
	type Output = I::Output;

	#[inline]
	fn index(&self, index: I) -> &Self::Output {
		self.0.index(index)
	}
}

impl<T, S, I> IndexMut<I> for BoundedVec<T, S>
where
	I: SliceIndex<[T]>,
{
	#[inline]
	fn index_mut(&mut self, index: I) -> &mut Self::Output {
		self.0.index_mut(index)
	}
}

impl<T, S> sp_std::iter::IntoIterator for BoundedVec<T, S> {
	type Item = T;
	type IntoIter = sp_std::vec::IntoIter<T>;
	fn into_iter(self) -> Self::IntoIter {
		self.0.into_iter()
	}
}

impl<T, S> codec::DecodeLength for BoundedVec<T, S> {
	fn len(self_encoded: &[u8]) -> Result<usize, codec::Error> {
		// `BoundedVec<T, _>` stored just a `Vec<T>`, thus the length is at the beginning in
		// `Compact` form, and same implementation as `Vec<T>` can be used.
		<Vec<T> as codec::DecodeLength>::len(self_encoded)
	}
}

// NOTE: we could also implement this as:
// impl<T: Value, S1: Get<u32>, S2: Get<u32>> PartialEq<BoundedVec<T, S2>> for BoundedVec<T, S1>
// to allow comparison of bounded vectors with different bounds.
impl<T, S> PartialEq for BoundedVec<T, S>
where
	T: PartialEq,
{
	fn eq(&self, rhs: &Self) -> bool {
		self.0 == rhs.0
	}
}

impl<T: PartialEq, S: Get<u32>> PartialEq<Vec<T>> for BoundedVec<T, S> {
	fn eq(&self, other: &Vec<T>) -> bool {
		&self.0 == other
	}
}

impl<T, S> Eq for BoundedVec<T, S> where T: Eq {}

impl<T, S> StorageDecodeLength for BoundedVec<T, S> {}

/// Storage value that is *maybe* capable of [`StorageAppend`](crate::storage::StorageAppend).
pub trait TryAppendValue<T: Encode, S: Get<u32>> {
	/// Try and append the `item` into the storage item.
	///
	/// This might fail if bounds are not respected.
	fn try_append<LikeT: EncodeLike<T>>(item: LikeT) -> Result<(), ()>;
}

/// Storage map that is *maybe* capable of [`StorageAppend`](crate::storage::StorageAppend).
pub trait TryAppendMap<K: FullCodec, T: Encode, S: Get<u32>> {
	/// Try and append the `item` into the storage map at the given `key`.
	///
	/// This might fail if bounds are not respected.
	fn try_append<LikeK: EncodeLike<K> + Clone, LikeT: EncodeLike<T>>(
		key: LikeK,
		item: LikeT,
	) -> Result<(), ()>;
}

/// Storage double map that is *maybe* capable of [`StorageAppend`](crate::storage::StorageAppend).
pub trait TryAppendDoubleMap<K1: FullCodec, K2: FullCodec, T: Encode, S: Get<u32>> {
	/// Try and append the `item` into the storage double map at the given `key`.
	///
	/// This might fail if bounds are not respected.
	fn try_append<
		LikeK1: EncodeLike<K1> + Clone,
		LikeK2: EncodeLike<K2> + Clone,
		LikeT: EncodeLike<T>,
	>(
		key1: LikeK1,
		key2: LikeK2,
		item: LikeT,
	) -> Result<(), ()>;
}

impl<T, S, StorageValueT> TryAppendValue<T, S> for StorageValueT
where
	BoundedVec<T, S>: FullCodec,
	T: Encode,
	S: Get<u32>,
	StorageValueT: generator::StorageValue<BoundedVec<T, S>>,
{
	fn try_append<LikeT: EncodeLike<T>>(item: LikeT) -> Result<(), ()> {
		let bound = BoundedVec::<T, S>::bound();
		let current = Self::decode_len().unwrap_or_default();
		if current < bound {
			// NOTE: we cannot reuse the implementation for `Vec<T>` here because we never want to
			// mark `BoundedVec<T, S>` as `StorageAppend`.
			let key = Self::storage_value_final_key();
			sp_io::storage::append(&key, item.encode());
			Ok(())
		} else {
			Err(())
		}
	}
}

impl<K, T, S, StorageMapT> TryAppendMap<K, T, S> for StorageMapT
where
	K: FullCodec,
	BoundedVec<T, S>: FullCodec,
	T: Encode,
	S: Get<u32>,
	StorageMapT: generator::StorageMap<K, BoundedVec<T, S>>,
{
	fn try_append<LikeK: EncodeLike<K> + Clone, LikeT: EncodeLike<T>>(
		key: LikeK,
		item: LikeT,
	) -> Result<(), ()> {
		let bound = BoundedVec::<T, S>::bound();
		let current = Self::decode_len(key.clone()).unwrap_or_default();
		if current < bound {
			let key = Self::storage_map_final_key(key);
			sp_io::storage::append(&key, item.encode());
			Ok(())
		} else {
			Err(())
		}
	}
}

impl<K1, K2, T, S, StorageDoubleMapT> TryAppendDoubleMap<K1, K2, T, S> for StorageDoubleMapT
where
	K1: FullCodec,
	K2: FullCodec,
	BoundedVec<T, S>: FullCodec,
	T: Encode,
	S: Get<u32>,
	StorageDoubleMapT: generator::StorageDoubleMap<K1, K2, BoundedVec<T, S>>,
{
	fn try_append<
		LikeK1: EncodeLike<K1> + Clone,
		LikeK2: EncodeLike<K2> + Clone,
		LikeT: EncodeLike<T>,
	>(
		key1: LikeK1,
		key2: LikeK2,
		item: LikeT,
	) -> Result<(), ()> {
		let bound = BoundedVec::<T, S>::bound();
		let current = Self::decode_len(key1.clone(), key2.clone()).unwrap_or_default();
		if current < bound {
			let double_map_key = Self::storage_double_map_final_key(key1, key2);
			sp_io::storage::append(&double_map_key, item.encode());
			Ok(())
		} else {
			Err(())
		}
	}
}

impl<T, S> MaxEncodedLen for BoundedVec<T, S>
where
	T: MaxEncodedLen,
	S: Get<u32>,
	BoundedVec<T, S>: Encode,
{
	fn max_encoded_len() -> usize {
		// BoundedVec<T, S> encodes like Vec<T> which encodes like [T], which is a compact u32
		// plus each item in the slice:
		// https://substrate.dev/rustdocs/v3.0.0/src/parity_scale_codec/codec.rs.html#798-808
		codec::Compact(S::get())
			.encoded_size()
			.saturating_add(Self::bound().saturating_mul(T::max_encoded_len()))
	}
}

#[cfg(test)]
pub mod test {
	use super::*;
	use sp_io::TestExternalities;
	use sp_std::convert::TryInto;
	use crate::{assert_ok, Twox128};

	crate::parameter_types! {
		pub const Seven: u32 = 7;
		pub const Four: u32 = 4;
	}

	crate::generate_storage_alias! { Prefix, Foo => Value<BoundedVec<u32, Seven>> }
	crate::generate_storage_alias! { Prefix, FooMap => Map<(u32, Twox128), BoundedVec<u32, Seven>> }
	crate::generate_storage_alias! {
		Prefix,
		FooDoubleMap => DoubleMap<(u32, Twox128), (u32, Twox128), BoundedVec<u32, Seven>>
	}

	#[test]
	fn decode_len_works() {
		TestExternalities::default().execute_with(|| {
			let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
			Foo::put(bounded);
			assert_eq!(Foo::decode_len().unwrap(), 3);
		});

		TestExternalities::default().execute_with(|| {
			let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
			FooMap::insert(1, bounded);
			assert_eq!(FooMap::decode_len(1).unwrap(), 3);
			assert!(FooMap::decode_len(0).is_none());
			assert!(FooMap::decode_len(2).is_none());
		});

		TestExternalities::default().execute_with(|| {
			let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
			FooDoubleMap::insert(1, 1, bounded);
			assert_eq!(FooDoubleMap::decode_len(1, 1).unwrap(), 3);
			assert!(FooDoubleMap::decode_len(2, 1).is_none());
			assert!(FooDoubleMap::decode_len(1, 2).is_none());
			assert!(FooDoubleMap::decode_len(2, 2).is_none());
		});
	}

	#[test]
	fn try_append_works() {
		TestExternalities::default().execute_with(|| {
			let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
			Foo::put(bounded);
			assert_ok!(Foo::try_append(4));
			assert_ok!(Foo::try_append(5));
			assert_ok!(Foo::try_append(6));
			assert_ok!(Foo::try_append(7));
			assert_eq!(Foo::decode_len().unwrap(), 7);
			assert!(Foo::try_append(8).is_err());
		});

		TestExternalities::default().execute_with(|| {
			let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
			FooMap::insert(1, bounded);

			assert_ok!(FooMap::try_append(1, 4));
			assert_ok!(FooMap::try_append(1, 5));
			assert_ok!(FooMap::try_append(1, 6));
			assert_ok!(FooMap::try_append(1, 7));
			assert_eq!(FooMap::decode_len(1).unwrap(), 7);
			assert!(FooMap::try_append(1, 8).is_err());

			// append to a non-existing
			assert!(FooMap::get(2).is_none());
			assert_ok!(FooMap::try_append(2, 4));
			assert_eq!(FooMap::get(2).unwrap(), unsafe {
				BoundedVec::<u32, Seven>::unchecked_from(vec![4])
			});
			assert_ok!(FooMap::try_append(2, 5));
			assert_eq!(FooMap::get(2).unwrap(), unsafe {
				BoundedVec::<u32, Seven>::unchecked_from(vec![4, 5])
			});
		});

		TestExternalities::default().execute_with(|| {
			let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
			FooDoubleMap::insert(1, 1, bounded);

			assert_ok!(FooDoubleMap::try_append(1, 1, 4));
			assert_ok!(FooDoubleMap::try_append(1, 1, 5));
			assert_ok!(FooDoubleMap::try_append(1, 1, 6));
			assert_ok!(FooDoubleMap::try_append(1, 1, 7));
			assert_eq!(FooDoubleMap::decode_len(1, 1).unwrap(), 7);
			assert!(FooDoubleMap::try_append(1, 1, 8).is_err());

			// append to a non-existing
			assert!(FooDoubleMap::get(2, 1).is_none());
			assert_ok!(FooDoubleMap::try_append(2, 1, 4));
			assert_eq!(FooDoubleMap::get(2, 1).unwrap(), unsafe {
				BoundedVec::<u32, Seven>::unchecked_from(vec![4])
			});
			assert_ok!(FooDoubleMap::try_append(2, 1, 5));
			assert_eq!(FooDoubleMap::get(2, 1).unwrap(), unsafe {
				BoundedVec::<u32, Seven>::unchecked_from(vec![4, 5])
			});
		});
	}

	#[test]
	fn try_insert_works() {
		let mut bounded: BoundedVec<u32, Four> = vec![1, 2, 3].try_into().unwrap();
		bounded.try_insert(1, 0).unwrap();
		assert_eq!(*bounded, vec![1, 0, 2, 3]);

		assert!(bounded.try_insert(0, 9).is_err());
		assert_eq!(*bounded, vec![1, 0, 2, 3]);
	}

	#[test]
	#[should_panic(expected = "insertion index (is 9) should be <= len (is 3)")]
	fn try_inert_panics_if_oob() {
		let mut bounded: BoundedVec<u32, Four> = vec![1, 2, 3].try_into().unwrap();
		bounded.try_insert(9, 0).unwrap();
	}

	#[test]
	fn try_push_works() {
		let mut bounded: BoundedVec<u32, Four> = vec![1, 2, 3].try_into().unwrap();
		bounded.try_push(0).unwrap();
		assert_eq!(*bounded, vec![1, 2, 3, 0]);

		assert!(bounded.try_push(9).is_err());
	}

	#[test]
	fn deref_coercion_works() {
		let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3].try_into().unwrap();
		// these methods come from deref-ed vec.
		assert_eq!(bounded.len(), 3);
		assert!(bounded.iter().next().is_some());
		assert!(!bounded.is_empty());
	}

	#[test]
	fn try_mutate_works() {
		let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3, 4, 5, 6].try_into().unwrap();
		let bounded = bounded.try_mutate(|v| v.push(7)).unwrap();
		assert_eq!(bounded.len(), 7);
		assert!(bounded.try_mutate(|v| v.push(8)).is_none());
	}

	#[test]
	fn slice_indexing_works() {
		let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3, 4, 5, 6].try_into().unwrap();
		assert_eq!(&bounded[0..=2], &[1, 2, 3]);
	}

	#[test]
	fn vec_eq_works() {
		let bounded: BoundedVec<u32, Seven> = vec![1, 2, 3, 4, 5, 6].try_into().unwrap();
		assert_eq!(bounded, vec![1, 2, 3, 4, 5, 6]);
	}
}
